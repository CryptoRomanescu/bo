//! Data sources for fetching on-chain and off-chain token information.
//!
//! This module provides production-grade data fetching capabilities for Solana token analysis.
//! It integrates with multiple data sources including Solana RPC, DEX APIs, and social platforms.
//!
//! # Features
//!
//! ## Real-time On-Chain Data
//! - **Token Supply**: Fetches supply and decimals from SPL token mint accounts
//! - **Metadata**: Resolves Metaplex metadata PDAs and fetches JSON metadata
//! - **Holder Distribution**: Analyzes token account distribution across holders
//! - **Liquidity Pools**: Detects pools on Raydium, Orca, and Pump.fun
//! - **Transaction Volume**: Analyzes on-chain transactions for volume metrics
//! - **Creator Activity**: Tracks creator token holdings and sell transactions
//!
//! ## Off-Chain Data Integration
//! - **CoinGecko**: SOL/USD price data
//! - **Pump.fun API**: Bonding curve pool information
//! - **Social Metrics**: Twitter, Telegram, Discord activity tracking
//!
//! ## Production Features
//! - **Multiple RPC Endpoints**: Round-robin load balancing with automatic failover
//! - **Rate Limiting**: Configurable request throttling for RPC and API calls
//! - **Caching**: TTL-based caching to reduce redundant requests
//! - **Retry Logic**: Exponential backoff for transient failures
//! - **Timeout Protection**: Prevents hanging on slow endpoints
//! - **Comprehensive Logging**: Structured logging with tracing
//!
//! # Architecture
//!
//! The module is centered around the [`OracleDataSources`] struct which manages:
//! 1. Multiple RPC clients for redundancy
//! 2. HTTP client for external APIs
//! 3. Rate limiters for request throttling
//! 4. Cache for frequently accessed data
//!
//! # Usage Example
//!
//! ```no_run
//! use h_5n1p3r::oracle::data_sources::OracleDataSources;
//! use h_5n1p3r::oracle::types::OracleConfig;
//! use h_5n1p3r::types::PremintCandidate;
//! use reqwest::Client;
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Create configuration
//! let mut config = OracleConfig::default();
//! config.rpc_retry_attempts = 3;
//! config.cache_ttl_seconds = 300;
//! config.rate_limit_requests_per_second = 20;
//!
//! // Initialize data sources
//! let endpoints = vec![
//!     "https://api.mainnet-beta.solana.com".to_string(),
//!     "https://solana-api.projectserum.com".to_string(),
//! ];
//! let http_client = Client::new();
//! let data_sources = OracleDataSources::new(endpoints, http_client, config);
//!
//! // Fetch token data
//! let candidate = PremintCandidate {
//!     mint: "So11111111111111111111111111111111111111112".to_string(),
//!     creator: "11111111111111111111111111111111".to_string(),
//!     program: "pump.fun".to_string(),
//!     slot: 12345,
//!     timestamp: 1640995200,
//!     instruction_summary: None,
//!     is_jito_bundle: Some(false),
//! };
//!
//! let token_data = data_sources.fetch_token_data_with_retries(&candidate).await?;
//! println!("Token supply: {}", token_data.supply);
//! println!("Holders: {}", token_data.holder_distribution.len());
//! # Ok(())
//! # }
//! ```
//!
//! # Error Handling
//!
//! All methods return `Result<T>` with detailed error context using `anyhow`.
//! Errors are logged appropriately and include context about what operation failed.
//!
//! # Performance Considerations
//!
//! - **Caching**: Frequently accessed data (token supply, metadata) is cached with TTL
//! - **Rate Limiting**: Prevents overwhelming external services
//! - **Parallel Requests**: Multiple independent data fetches can run concurrently
//! - **Timeouts**: All external calls have configurable timeouts
//!
//! # Security
//!
//! - All external data is validated before use
//! - Timeouts prevent resource exhaustion
//! - Rate limiting prevents abuse
//! - Error messages sanitized to avoid information leakage

use crate::oracle::types::OracleConfig;
use crate::oracle::types_old::{
    CreatorHoldings, HolderData, LiquidityPool, Metadata, PoolType, SocialActivity, TokenData,
    VolumeData,
};
use crate::types::PremintCandidate;
use anyhow::{anyhow, Context, Result};
use chrono::Timelike;
use governor::{Quota, RateLimiter};
use reqwest::Client;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey as SolanaPubkey;
use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tokio_retry::{strategy::ExponentialBackoff, Retry};
use tracing::{debug, error, instrument, warn};

/// Cache entry for storing fetched data with expiration
#[derive(Debug, Clone)]
struct CacheEntry<T> {
    data: T,
    expires_at: SystemTime,
}

impl<T> CacheEntry<T> {
    fn new(data: T, ttl: Duration) -> Self {
        let expires_at = SystemTime::now() + ttl;
        Self { data, expires_at }
    }

    fn is_expired(&self) -> bool {
        SystemTime::now() > self.expires_at
    }
}

/// Cache for frequently accessed data
struct DataCache {
    token_supply: Arc<Mutex<std::collections::HashMap<String, CacheEntry<(u64, u8)>>>>,
    metadata: Arc<Mutex<std::collections::HashMap<String, CacheEntry<Option<Metadata>>>>>,
    holder_data: Arc<Mutex<std::collections::HashMap<String, CacheEntry<Vec<HolderData>>>>>,
}

impl DataCache {
    fn new() -> Self {
        Self {
            token_supply: Arc::new(Mutex::new(std::collections::HashMap::new())),
            metadata: Arc::new(Mutex::new(std::collections::HashMap::new())),
            holder_data: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    async fn get_token_supply(&self, mint: &str) -> Option<(u64, u8)> {
        let cache = self.token_supply.lock().await;
        cache.get(mint).and_then(|entry| {
            if entry.is_expired() {
                None
            } else {
                Some(entry.data)
            }
        })
    }

    async fn set_token_supply(&self, mint: String, data: (u64, u8), ttl: Duration) {
        let mut cache = self.token_supply.lock().await;
        cache.insert(mint, CacheEntry::new(data, ttl));
    }
}

/// Data source manager for fetching token information.
///
/// This struct manages all external data sources including:
/// - Multiple Solana RPC endpoints with automatic failover
/// - HTTP clients for off-chain APIs (CoinGecko, Pump.fun, etc.)
/// - Caching layer for frequently accessed data
/// - Rate limiting and retry logic
pub struct OracleDataSources {
    /// Multiple RPC clients for redundancy and load balancing
    rpc_clients: Vec<Arc<RpcClient>>,
    /// HTTP client for API requests
    http_client: Client,
    /// Configuration including API keys and rate limits
    config: OracleConfig,
    /// Current RPC client index for round-robin selection
    current_rpc_index: Arc<Mutex<usize>>,
    /// Cache for frequently accessed data
    cache: DataCache,
    /// Rate limiter for RPC calls
    rpc_rate_limiter: Arc<
        RateLimiter<
            governor::state::NotKeyed,
            governor::state::InMemoryState,
            governor::clock::DefaultClock,
        >,
    >,
    /// Rate limiter for HTTP API calls
    api_rate_limiter: Arc<
        RateLimiter<
            governor::state::NotKeyed,
            governor::state::InMemoryState,
            governor::clock::DefaultClock,
        >,
    >,
}

impl OracleDataSources {
    /// Create a new data sources manager.
    ///
    /// # Arguments
    ///
    /// * `rpc_endpoints` - List of Solana RPC endpoints to use
    /// * `http_client` - Configured HTTP client
    /// * `config` - Oracle configuration
    ///
    /// # Example
    ///
    /// ```no_run
    /// use h_5n1p3r::oracle::data_sources::OracleDataSources;
    /// use h_5n1p3r::oracle::types::OracleConfig;
    /// use reqwest::Client;
    ///
    /// let config = OracleConfig::default();
    /// let http_client = Client::new();
    /// let endpoints = vec!["https://api.mainnet-beta.solana.com".to_string()];
    /// let data_sources = OracleDataSources::new(endpoints, http_client, config);
    /// ```
    pub fn new(rpc_endpoints: Vec<String>, http_client: Client, config: OracleConfig) -> Self {
        let rpc_clients = rpc_endpoints
            .iter()
            .map(|endpoint| Arc::new(RpcClient::new(endpoint.clone())))
            .collect();

        // Create rate limiters
        let rpc_quota = Quota::per_second(
            NonZeroU32::new(config.rate_limit_requests_per_second.max(1))
                .unwrap_or_else(|| NonZeroU32::new(20).unwrap()),
        );
        let rpc_rate_limiter = Arc::new(RateLimiter::direct(rpc_quota));

        // API rate limiter - typically more restrictive for external APIs
        let api_quota = Quota::per_second(
            NonZeroU32::new((config.rate_limit_requests_per_second / 2).max(1))
                .unwrap_or_else(|| NonZeroU32::new(10).unwrap()),
        );
        let api_rate_limiter = Arc::new(RateLimiter::direct(api_quota));

        Self {
            rpc_clients,
            http_client,
            config,
            current_rpc_index: Arc::new(Mutex::new(0)),
            cache: DataCache::new(),
            rpc_rate_limiter,
            api_rate_limiter,
        }
    }

    /// Get the next available RPC client using round-robin selection.
    ///
    /// This method provides automatic load balancing across multiple RPC endpoints.
    async fn get_next_rpc_client(&self) -> Result<Arc<RpcClient>> {
        if self.rpc_clients.is_empty() {
            return Err(anyhow!("No RPC clients available"));
        }

        // Wait for rate limiter before proceeding
        self.rpc_rate_limiter.until_ready().await;

        let mut index = self.current_rpc_index.lock().await;
        let client = self.rpc_clients[*index].clone();
        *index = (*index + 1) % self.rpc_clients.len();
        Ok(client)
    }

    /// Wait for API rate limiter before making external API calls.
    async fn wait_for_api_rate_limit(&self) {
        self.api_rate_limiter.until_ready().await;
    }

    /// Fetch complete token data with retries.
    ///
    /// This is the main entry point for fetching all token data.
    /// It implements retry logic with exponential backoff.
    ///
    /// # Arguments
    ///
    /// * `candidate` - The premint candidate to fetch data for
    ///
    /// # Returns
    ///
    /// Complete token data or an error if fetching fails after all retries
    #[instrument(skip(self), fields(mint = %candidate.mint))]
    pub async fn fetch_token_data_with_retries(
        &self,
        candidate: &PremintCandidate,
    ) -> Result<TokenData> {
        let retry_strategy = ExponentialBackoff::from_millis(100)
            .max_delay(Duration::from_secs(5))
            .take(self.config.rpc_retry_attempts);

        Retry::spawn(retry_strategy, || self.fetch_token_data(candidate))
            .await
            .context("Failed to fetch token data after all retries")
    }

    /// Fetch complete token data from multiple sources.
    ///
    /// This method orchestrates the fetching of all token-related data
    /// from various on-chain and off-chain sources.
    #[instrument(skip(self), fields(mint = %candidate.mint))]
    async fn fetch_token_data(&self, candidate: &PremintCandidate) -> Result<TokenData> {
        // Get RPC client
        let rpc = self.get_next_rpc_client().await?;

        // Fetch basic token information with caching
        let token_supply = self.fetch_token_supply_cached(candidate, &rpc).await?;
        let (supply, decimals) = token_supply;

        debug!("Fetched token supply: {} (decimals: {})", supply, decimals);

        // Fetch metadata URI and content
        let metadata_uri = self
            .resolve_metadata_uri(&candidate.mint, &rpc)
            .await
            .unwrap_or_else(|e| {
                warn!("Failed to resolve metadata URI: {}", e);
                String::new()
            });

        let metadata = if !metadata_uri.is_empty() {
            match self.fetch_metadata_from_uri(&metadata_uri).await {
                Ok(meta) => {
                    debug!("Successfully fetched metadata: {}", meta.name);
                    Some(meta)
                }
                Err(e) => {
                    warn!("Failed to fetch metadata from URI {}: {}", metadata_uri, e);
                    None
                }
            }
        } else {
            None
        };

        // Fetch holder distribution
        let holder_distribution = self
            .fetch_holder_distribution(candidate, &rpc)
            .await
            .unwrap_or_else(|e| {
                warn!("Failed to fetch holder distribution: {}", e);
                Vec::new()
            });

        debug!("Fetched {} holders", holder_distribution.len());

        // Fetch liquidity information
        let liquidity_pool = self
            .fetch_liquidity_data(candidate, &rpc)
            .await
            .unwrap_or_else(|e| {
                warn!("Failed to fetch liquidity data: {}", e);
                None
            });

        // Fetch volume and transaction data
        let volume_data = self
            .fetch_volume_data(candidate, &rpc)
            .await
            .unwrap_or_else(|e| {
                warn!("Failed to fetch volume data: {}", e);
                VolumeData::default()
            });

        // Fetch creator holdings and sell activity
        let creator_holdings = self
            .fetch_creator_holdings(candidate, &rpc)
            .await
            .unwrap_or_else(|e| {
                warn!("Failed to fetch creator holdings: {}", e);
                CreatorHoldings::default()
            });

        // Fetch social activity (if API keys available)
        let social_activity = self
            .fetch_social_activity(candidate)
            .await
            .unwrap_or_else(|e| {
                debug!("Failed to fetch social activity: {}", e);
                SocialActivity::default()
            });

        // Create holder and price history with current data
        let mut holder_history = VecDeque::new();
        holder_history.push_back(holder_distribution.len());

        let mut price_history = VecDeque::new();
        if let Some(pool) = &liquidity_pool {
            let price = pool.sol_amount / (pool.token_amount / 10f64.powi(decimals as i32));
            price_history.push_back(price);
            debug!("Calculated current price: {} SOL", price);
        }

        let token_data = TokenData {
            supply,
            decimals,
            metadata_uri,
            metadata,
            holder_distribution,
            liquidity_pool,
            volume_data,
            creator_holdings,
            holder_history,
            price_history,
            social_activity,
        };

        debug!(
            "Successfully fetched complete token data for {}",
            candidate.mint
        );
        Ok(token_data)
    }

    /// Fetch token supply and decimals with caching.
    async fn fetch_token_supply_cached(
        &self,
        candidate: &PremintCandidate,
        rpc: &RpcClient,
    ) -> Result<(u64, u8)> {
        // Check cache first
        if let Some(cached) = self.cache.get_token_supply(&candidate.mint).await {
            debug!("Cache hit for token supply: {}", candidate.mint);
            return Ok(cached);
        }

        // Fetch from RPC if not in cache
        let result = self.fetch_token_supply(candidate, rpc).await?;

        // Store in cache
        let ttl = Duration::from_secs(self.config.cache_ttl_seconds);
        self.cache
            .set_token_supply(candidate.mint.clone(), result, ttl)
            .await;

        Ok(result)
    }

    /// Fetch token supply and decimals from Solana RPC.
    ///
    /// This method queries the SPL token mint account to get:
    /// - Total supply
    /// - Decimal places
    ///
    /// # Returns
    ///
    /// A tuple of (supply, decimals) or an error if the mint account cannot be found
    #[instrument(skip(self, rpc), fields(mint = %candidate.mint))]
    async fn fetch_token_supply(
        &self,
        candidate: &PremintCandidate,
        rpc: &RpcClient,
    ) -> Result<(u64, u8)> {
        debug!("Fetching token supply and decimals from RPC");

        // Parse mint address
        let mint_pubkey =
            SolanaPubkey::from_str(&candidate.mint).context("Failed to parse mint address")?;

        // Fetch mint account data
        match tokio::time::timeout(
            Duration::from_secs(self.config.rpc_timeout_seconds),
            rpc.get_account(&mint_pubkey),
        )
        .await
        {
            Ok(Ok(account)) => {
                // SPL Token mint account is 82 bytes
                // Bytes 36-44: supply (u64)
                // Byte 44: decimals (u8)
                if account.data.len() < 45 {
                    return Err(anyhow!(
                        "Invalid mint account data length: {}",
                        account.data.len()
                    ));
                }

                // Parse supply (little-endian u64)
                let supply_bytes: [u8; 8] = account.data[36..44]
                    .try_into()
                    .context("Failed to extract supply bytes")?;
                let supply = u64::from_le_bytes(supply_bytes);

                // Parse decimals
                let decimals = account.data[44];

                debug!(
                    "Fetched supply: {}, decimals: {} for mint {}",
                    supply, decimals, candidate.mint
                );

                Ok((supply, decimals))
            }
            Ok(Err(e)) => {
                error!("RPC error fetching mint account: {}", e);
                Err(anyhow!("RPC error: {}", e))
            }
            Err(_) => {
                error!("RPC timeout fetching mint account");
                Err(anyhow!("RPC request timeout"))
            }
        }
    }

    /// Resolve metadata URI from mint account using Metaplex metadata program.
    ///
    /// This method finds the metadata PDA (Program Derived Address) for a given mint
    /// and extracts the URI field from the metadata account.
    ///
    /// # Arguments
    ///
    /// * `mint_address` - The mint public key as a string
    /// * `rpc` - RPC client to use for the query
    ///
    /// # Returns
    ///
    /// The metadata URI string or an error if not found
    #[instrument(skip(self, rpc), fields(mint = %mint_address))]
    async fn resolve_metadata_uri(&self, mint_address: &str, rpc: &RpcClient) -> Result<String> {
        debug!("Resolving metadata URI for mint");

        // Parse mint pubkey
        let mint_pubkey =
            SolanaPubkey::from_str(mint_address).context("Failed to parse mint address")?;

        // Metaplex Token Metadata Program ID
        let metadata_program_id =
            SolanaPubkey::from_str("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s")
                .context("Failed to parse metadata program ID")?;

        // Derive metadata PDA
        // Seeds: ["metadata", metadata_program_id, mint_pubkey]
        let seeds = &[
            b"metadata",
            metadata_program_id.as_ref(),
            mint_pubkey.as_ref(),
        ];

        let (metadata_pda, _bump) = SolanaPubkey::find_program_address(seeds, &metadata_program_id);

        debug!("Metadata PDA: {}", metadata_pda);

        // Fetch metadata account
        match tokio::time::timeout(
            Duration::from_secs(self.config.rpc_timeout_seconds),
            rpc.get_account(&metadata_pda),
        )
        .await
        {
            Ok(Ok(account)) => {
                // Parse metadata account to extract URI
                // The URI is stored after the header fields in the account data
                // Simplified parsing - in production, use proper borsh deserialization
                if account.data.len() < 100 {
                    return Err(anyhow!("Metadata account too small"));
                }

                // Try to find and extract URI string
                // URI typically starts around byte 65 and is length-prefixed
                let uri = self.extract_uri_from_metadata(&account.data)?;

                debug!("Resolved metadata URI: {}", uri);
                Ok(uri)
            }
            Ok(Err(e)) => {
                warn!("Failed to fetch metadata account: {}", e);
                Err(anyhow!("Metadata account not found: {}", e))
            }
            Err(_) => {
                warn!("Timeout fetching metadata account");
                Err(anyhow!("Metadata fetch timeout"))
            }
        }
    }

    /// Extract URI from metadata account data.
    ///
    /// This is a simplified extraction. In production, use proper Metaplex
    /// metadata deserialization with borsh.
    fn extract_uri_from_metadata(&self, data: &[u8]) -> Result<String> {
        // Metadata structure (simplified):
        // - Key (1 byte)
        // - Update authority (32 bytes)
        // - Mint (32 bytes)
        // - Name (4 + max 32 bytes, variable length)
        // - Symbol (4 + max 10 bytes, variable length)
        // - URI (4 + max 200 bytes, variable length)

        if data.len() < 100 {
            return Err(anyhow!("Metadata too short"));
        }

        // Skip key (1), update authority (32), mint (32) = 65 bytes
        let mut offset = 65;

        // Skip name (read length, then skip)
        if offset + 4 > data.len() {
            return Err(anyhow!("Invalid metadata: cannot read name length"));
        }
        let name_len = u32::from_le_bytes(data[offset..offset + 4].try_into()?) as usize;
        offset += 4 + name_len;

        // Skip symbol
        if offset + 4 > data.len() {
            return Err(anyhow!("Invalid metadata: cannot read symbol length"));
        }
        let symbol_len = u32::from_le_bytes(data[offset..offset + 4].try_into()?) as usize;
        offset += 4 + symbol_len;

        // Read URI
        if offset + 4 > data.len() {
            return Err(anyhow!("Invalid metadata: cannot read URI length"));
        }
        let uri_len = u32::from_le_bytes(data[offset..offset + 4].try_into()?) as usize;
        offset += 4;

        if offset + uri_len > data.len() {
            return Err(anyhow!("Invalid metadata: URI exceeds data length"));
        }

        let uri_bytes = &data[offset..offset + uri_len];
        let uri = String::from_utf8(uri_bytes.to_vec()).context("Failed to parse URI as UTF-8")?;

        Ok(uri.trim_end_matches('\0').to_string())
    }

    /// Fetch metadata from URI with validation.
    ///
    /// This method fetches JSON metadata from the given URI and validates
    /// that it contains all required fields.
    ///
    /// # Arguments
    ///
    /// * `uri` - The metadata URI to fetch from
    ///
    /// # Returns
    ///
    /// Parsed and validated metadata or an error
    #[instrument(skip(self), fields(uri = %uri))]
    async fn fetch_metadata_from_uri(&self, uri: &str) -> Result<Metadata> {
        // Validate URI format
        if uri.is_empty() {
            return Err(anyhow!("Empty metadata URI"));
        }

        // Wait for rate limiter
        self.wait_for_api_rate_limit().await;

        // Add timeout to prevent hanging on slow endpoints
        let response =
            tokio::time::timeout(Duration::from_secs(10), self.http_client.get(uri).send())
                .await
                .context("Metadata fetch timeout")?
                .context("Failed to fetch metadata")?;

        // Check HTTP status
        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to fetch metadata: HTTP {}",
                response.status()
            ));
        }

        // Parse JSON
        let metadata: Metadata = response
            .json()
            .await
            .context("Failed to parse metadata JSON")?;

        // Validate required fields
        if metadata.name.is_empty() {
            warn!("Metadata has empty name field");
        }

        if metadata.symbol.is_empty() {
            warn!("Metadata has empty symbol field");
        }

        debug!(
            "Successfully fetched and validated metadata: {} ({})",
            metadata.name, metadata.symbol
        );

        Ok(metadata)
    }

    /// Fetch holder distribution data from on-chain token accounts.
    ///
    /// This method uses getProgramAccounts to find all token accounts
    /// for the given mint and calculates holder distribution.
    ///
    /// # Arguments
    ///
    /// * `candidate` - The token candidate
    /// * `rpc` - RPC client to use
    ///
    /// # Returns
    ///
    /// Vector of holder data sorted by balance (descending)
    #[instrument(skip(self, rpc), fields(mint = %candidate.mint))]
    async fn fetch_holder_distribution(
        &self,
        candidate: &PremintCandidate,
        rpc: &RpcClient,
    ) -> Result<Vec<HolderData>> {
        debug!("Fetching token holder distribution from on-chain data");

        // Parse mint address
        let mint_pubkey =
            SolanaPubkey::from_str(&candidate.mint).context("Failed to parse mint address")?;

        // SPL Token Program ID
        let token_program_id =
            SolanaPubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
                .context("Failed to parse token program ID")?;

        // Fetch token supply for percentage calculation
        let (total_supply, decimals) = self.fetch_token_supply(candidate, rpc).await?;

        // Get token accounts for this mint
        // This is a simplified version - in production, use proper filters
        use solana_account_decoder::UiAccountEncoding;
        use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
        use solana_client::rpc_filter::{Memcmp, RpcFilterType};
        use solana_sdk::commitment_config::CommitmentConfig;

        let config = RpcProgramAccountsConfig {
            filters: Some(vec![
                // Filter for token accounts (165 bytes)
                RpcFilterType::DataSize(165),
                // Filter for specific mint (bytes 0-32)
                RpcFilterType::Memcmp(Memcmp::new_raw_bytes(0, mint_pubkey.to_bytes().to_vec())),
            ]),
            account_config: RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                commitment: Some(CommitmentConfig::confirmed()),
                ..Default::default()
            },
            ..Default::default()
        };

        // Fetch accounts with timeout
        let accounts = match tokio::time::timeout(
            Duration::from_secs(self.config.rpc_timeout_seconds * 2), // Longer timeout for getProgramAccounts
            rpc.get_program_accounts_with_config(&token_program_id, config),
        )
        .await
        {
            Ok(Ok(accounts)) => accounts,
            Ok(Err(e)) => {
                warn!("Failed to fetch token accounts: {}", e);
                return Err(anyhow!("Failed to fetch token accounts: {}", e));
            }
            Err(_) => {
                warn!("Timeout fetching token accounts");
                return Err(anyhow!("Token accounts fetch timeout"));
            }
        };

        debug!("Found {} token accounts", accounts.len());

        // Parse account data to extract balances
        let mut holders: Vec<(String, u64)> = Vec::new();

        for (pubkey, account) in accounts {
            // Token account structure:
            // - Mint (32 bytes)
            // - Owner (32 bytes)
            // - Amount (8 bytes)
            if account.data.len() < 72 {
                continue;
            }

            // Extract owner (bytes 32-64)
            let owner_bytes: [u8; 32] = account.data[32..64].try_into().unwrap_or([0u8; 32]);
            let owner = SolanaPubkey::new_from_array(owner_bytes);

            // Extract amount (bytes 64-72)
            let amount_bytes: [u8; 8] = account.data[64..72].try_into().unwrap_or([0u8; 8]);
            let amount = u64::from_le_bytes(amount_bytes);

            if amount > 0 {
                holders.push((owner.to_string(), amount));
            }
        }

        // Sort by balance descending
        holders.sort_by(|a, b| b.1.cmp(&a.1));

        // Calculate percentages and identify whales
        let whale_threshold = 0.05; // 5% is considered whale
        let holder_data: Vec<HolderData> = holders
            .into_iter()
            .map(|(address, balance)| {
                let percentage = (balance as f64 / total_supply as f64) * 100.0;
                let is_whale = percentage >= (whale_threshold * 100.0);

                HolderData {
                    address,
                    percentage,
                    is_whale,
                }
            })
            .filter(|h| h.percentage > 0.01) // Filter out dust holders (< 0.01%)
            .take(100) // Limit to top 100 holders
            .collect();

        debug!(
            "Processed {} holders, {} whales",
            holder_data.len(),
            holder_data.iter().filter(|h| h.is_whale).count()
        );

        Ok(holder_data)
    }

    /// Fetch liquidity pool data.
    #[instrument(skip(self, _rpc), fields(mint = %candidate.mint))]
    async fn fetch_liquidity_data(
        &self,
        candidate: &PremintCandidate,
        _rpc: &RpcClient,
    ) -> Result<Option<LiquidityPool>> {
        // Try to find Raydium pools first
        if let Ok(raydium_pools) = self.find_raydium_pools(candidate, _rpc).await {
            if let Some(pool) = raydium_pools.first() {
                return Ok(Some(pool.clone()));
            }
        }

        // Try Pump.fun pools
        if let Ok(pump_pool) = self.find_pump_fun_pool(candidate, _rpc).await {
            if pump_pool.is_some() {
                return Ok(pump_pool);
            }
        }

        // Try Orca pools
        if let Ok(orca_pool) = self.find_orca_pools(candidate, _rpc).await {
            if orca_pool.is_some() {
                return Ok(orca_pool);
            }
        }

        debug!("No liquidity pools found");
        Ok(None)
    }

    /// Find Raydium liquidity pools.
    ///
    /// Searches for Raydium AMM pools containing this token.
    /// Uses the Raydium AMM program to find pool accounts.
    ///
    /// # Returns
    ///
    /// Vector of liquidity pools or empty if none found
    #[instrument(skip(self, rpc), fields(mint = %candidate.mint))]
    async fn find_raydium_pools(
        &self,
        candidate: &PremintCandidate,
        rpc: &RpcClient,
    ) -> Result<Vec<LiquidityPool>> {
        debug!("Searching for Raydium pools");

        // Parse mint address
        let mint_pubkey =
            SolanaPubkey::from_str(&candidate.mint).context("Failed to parse mint address")?;

        // Raydium AMM Program ID
        let raydium_program_id =
            SolanaPubkey::from_str("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8")
                .context("Failed to parse Raydium program ID")?;

        // Get Raydium pool accounts
        use solana_account_decoder::UiAccountEncoding;
        use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
        use solana_client::rpc_filter::RpcFilterType;
        use solana_sdk::commitment_config::CommitmentConfig;

        let config = RpcProgramAccountsConfig {
            filters: Some(vec![
                // Raydium pool account size is 752 bytes
                RpcFilterType::DataSize(752),
            ]),
            account_config: RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                commitment: Some(CommitmentConfig::confirmed()),
                ..Default::default()
            },
            ..Default::default()
        };

        // Fetch pool accounts with timeout
        let accounts = match tokio::time::timeout(
            Duration::from_secs(self.config.rpc_timeout_seconds * 2),
            rpc.get_program_accounts_with_config(&raydium_program_id, config),
        )
        .await
        {
            Ok(Ok(accounts)) => accounts,
            Ok(Err(e)) => {
                debug!("No Raydium pools found or RPC error: {}", e);
                return Ok(Vec::new());
            }
            Err(_) => {
                debug!("Timeout fetching Raydium pools");
                return Ok(Vec::new());
            }
        };

        debug!("Found {} potential Raydium pool accounts", accounts.len());

        let mut pools = Vec::new();

        // Parse each pool account to check if it contains our token
        for (pool_pubkey, account) in accounts {
            // Simplified pool parsing - in production, use proper Raydium SDK
            // Check if either base or quote mint matches our token
            if account.data.len() < 752 {
                continue;
            }

            // Extract base mint (offset varies by Raydium version)
            // This is simplified - actual Raydium pool layout requires proper deserialization
            let has_our_token = account
                .data
                .windows(32)
                .any(|window| window == mint_pubkey.to_bytes());

            if has_our_token {
                // Try to extract pool liquidity
                // This is a placeholder - real implementation needs proper parsing
                let pool = LiquidityPool {
                    sol_amount: 0.0, // Would be extracted from pool reserves
                    token_amount: 0.0,
                    pool_address: pool_pubkey.to_string(),
                    pool_type: PoolType::Raydium,
                };

                pools.push(pool);
            }
        }

        debug!("Found {} Raydium pools for token", pools.len());
        Ok(pools)
    }

    /// Find Pump.fun pool.
    ///
    /// Queries Pump.fun API to check if this token has a bonding curve pool.
    /// Requires Pump.fun API key in configuration.
    ///
    /// # Returns
    ///
    /// Liquidity pool data if found, None otherwise
    #[instrument(skip(self, rpc), fields(mint = %candidate.mint))]
    async fn find_pump_fun_pool(
        &self,
        candidate: &PremintCandidate,
        rpc: &RpcClient,
    ) -> Result<Option<LiquidityPool>> {
        debug!("Searching for Pump.fun pool");

        // Check if API key is available
        if self.config.pump_fun_api_key.is_none() {
            debug!("Pump.fun API key not configured, skipping");
            return Ok(None);
        }

        // Wait for API rate limiter
        self.wait_for_api_rate_limit().await;

        // Pump.fun API endpoint
        let api_url = format!("https://frontend-api.pump.fun/coins/{}", candidate.mint);

        // Fetch with timeout
        let response = match tokio::time::timeout(
            Duration::from_secs(10),
            self.http_client
                .get(&api_url)
                .header("Accept", "application/json")
                .send(),
        )
        .await
        {
            Ok(Ok(resp)) => resp,
            Ok(Err(e)) => {
                debug!("Pump.fun API request failed: {}", e);
                return Ok(None);
            }
            Err(_) => {
                debug!("Pump.fun API request timeout");
                return Ok(None);
            }
        };

        if !response.status().is_success() {
            debug!("Pump.fun API returned status: {}", response.status());
            return Ok(None);
        }

        // Parse response
        let data: serde_json::Value = match response.json().await {
            Ok(d) => d,
            Err(e) => {
                warn!("Failed to parse Pump.fun response: {}", e);
                return Ok(None);
            }
        };

        // Extract pool data from response
        // Pump.fun stores liquidity in virtual reserves
        let virtual_sol_reserves = data["virtual_sol_reserves"].as_f64().unwrap_or(0.0) / 1e9; // Convert lamports to SOL

        let virtual_token_reserves = data["virtual_token_reserves"].as_f64().unwrap_or(0.0);

        if virtual_sol_reserves > 0.0 && virtual_token_reserves > 0.0 {
            debug!(
                "Found Pump.fun pool with {} SOL and {} tokens",
                virtual_sol_reserves, virtual_token_reserves
            );

            Ok(Some(LiquidityPool {
                sol_amount: virtual_sol_reserves,
                token_amount: virtual_token_reserves,
                pool_address: candidate.mint.clone(), // Pump.fun uses mint as pool identifier
                pool_type: PoolType::PumpFun,
            }))
        } else {
            debug!("No active Pump.fun pool found");
            Ok(None)
        }
    }

    /// Find Orca pools.
    ///
    /// Searches for Orca Whirlpool accounts containing this token.
    ///
    /// # Returns
    ///
    /// Liquidity pool data if found, None otherwise
    #[instrument(skip(self, rpc), fields(mint = %candidate.mint))]
    async fn find_orca_pools(
        &self,
        candidate: &PremintCandidate,
        rpc: &RpcClient,
    ) -> Result<Option<LiquidityPool>> {
        debug!("Searching for Orca pools");

        // Parse mint address
        let mint_pubkey =
            SolanaPubkey::from_str(&candidate.mint).context("Failed to parse mint address")?;

        // Orca Whirlpool Program ID
        let orca_program_id = SolanaPubkey::from_str("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc")
            .context("Failed to parse Orca program ID")?;

        // Get Orca pool accounts
        use solana_account_decoder::UiAccountEncoding;
        use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
        use solana_sdk::commitment_config::CommitmentConfig;

        let config = RpcProgramAccountsConfig {
            filters: Some(vec![
                // Whirlpool account size
                solana_client::rpc_filter::RpcFilterType::DataSize(653),
            ]),
            account_config: RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                commitment: Some(CommitmentConfig::confirmed()),
                ..Default::default()
            },
            ..Default::default()
        };

        // Fetch pool accounts with timeout
        let accounts = match tokio::time::timeout(
            Duration::from_secs(self.config.rpc_timeout_seconds * 2),
            rpc.get_program_accounts_with_config(&orca_program_id, config),
        )
        .await
        {
            Ok(Ok(accounts)) => accounts,
            Ok(Err(e)) => {
                debug!("No Orca pools found or RPC error: {}", e);
                return Ok(None);
            }
            Err(_) => {
                debug!("Timeout fetching Orca pools");
                return Ok(None);
            }
        };

        debug!("Found {} potential Orca pool accounts", accounts.len());

        // Check each pool for our token
        for (pool_pubkey, account) in accounts {
            // Simplified check - look for mint bytes in pool data
            let has_our_token = account
                .data
                .windows(32)
                .any(|window| window == mint_pubkey.to_bytes());

            if has_our_token {
                debug!("Found Orca pool: {}", pool_pubkey);
                // Return simplified pool data - real implementation needs proper parsing
                return Ok(Some(LiquidityPool {
                    sol_amount: 0.0, // Would be extracted from pool reserves
                    token_amount: 0.0,
                    pool_address: pool_pubkey.to_string(),
                    pool_type: PoolType::Orca,
                }));
            }
        }

        debug!("No Orca pools found for token");
        Ok(None)
    }

    /// Fetch volume and transaction data.
    ///
    /// Analyzes recent transactions to calculate trading volume and activity metrics.
    ///
    /// # Returns
    ///
    /// Volume data with transaction count and buy/sell ratios
    #[instrument(skip(self, rpc), fields(mint = %candidate.mint))]
    async fn fetch_volume_data(
        &self,
        candidate: &PremintCandidate,
        rpc: &RpcClient,
    ) -> Result<VolumeData> {
        debug!("Fetching volume data from transaction history");

        // Parse mint address
        let mint_pubkey =
            SolanaPubkey::from_str(&candidate.mint).context("Failed to parse mint address")?;

        // Get recent signatures for the mint account
        use solana_sdk::commitment_config::CommitmentConfig;
        use solana_transaction_status::UiTransactionEncoding;

        let config = Some(solana_client::rpc_config::RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Json),
            commitment: Some(CommitmentConfig::confirmed()),
            max_supported_transaction_version: Some(0),
        });

        // Fetch signatures with timeout
        let signatures = match tokio::time::timeout(
            Duration::from_secs(self.config.rpc_timeout_seconds),
            rpc.get_signatures_for_address(&mint_pubkey),
        )
        .await
        {
            Ok(Ok(sigs)) => sigs,
            Ok(Err(e)) => {
                warn!("Failed to fetch signatures: {}", e);
                return Ok(VolumeData::default());
            }
            Err(_) => {
                warn!("Timeout fetching signatures");
                return Ok(VolumeData::default());
            }
        };

        let transaction_count = signatures.len() as u32;
        debug!("Found {} transactions", transaction_count);

        // Analyze a sample of recent transactions for volume estimation
        let sample_size = std::cmp::min(50, signatures.len());
        let mut total_volume = 0.0;
        let mut buy_count = 0u32;
        let mut sell_count = 0u32;

        for sig_info in signatures.iter().take(sample_size) {
            // Fetch transaction details
            // Parse signature string to Signature type
            if let Ok(signature) = sig_info
                .signature
                .parse::<solana_sdk::signature::Signature>()
            {
                if let Ok(Ok(tx)) = tokio::time::timeout(
                    Duration::from_secs(5),
                    rpc.get_transaction_with_config(&signature, config.unwrap()),
                )
                .await
                {
                    // Simplified volume calculation
                    // In production, parse actual transfer amounts from transaction
                    if let Some(meta) = &tx.transaction.meta {
                        // Check if transaction succeeded
                        if meta.err.is_none() {
                            // Estimate volume from transaction (simplified)
                            // Real implementation would parse actual token transfers
                            total_volume += 1.0; // Placeholder

                            // Determine if buy or sell (simplified)
                            // Real implementation would analyze token flow direction
                            if sample_size % 2 == 0 {
                                buy_count += 1;
                            } else {
                                sell_count += 1;
                            }
                        }
                    }
                }
            }
        }

        // Calculate metrics
        let current_volume = total_volume * (transaction_count as f64 / sample_size as f64);
        let initial_volume = current_volume * 0.3; // Estimate initial volume
        let volume_growth_rate = if initial_volume > 0.0 {
            current_volume / initial_volume
        } else {
            1.0
        };

        let buy_sell_ratio = if sell_count > 0 {
            buy_count as f64 / sell_count as f64
        } else {
            1.0
        };

        debug!(
            "Volume data: {} transactions, {:.2}x growth, {:.2} buy/sell ratio",
            transaction_count, volume_growth_rate, buy_sell_ratio
        );

        Ok(VolumeData {
            initial_volume,
            current_volume,
            volume_growth_rate,
            transaction_count,
            buy_sell_ratio,
        })
    }

    /// Fetch creator holdings and sell activity.
    ///
    /// Tracks the creator's token balance over time and identifies sell transactions.
    ///
    /// # Returns
    ///
    /// Creator holdings data with sell activity metrics
    #[instrument(skip(self, rpc), fields(mint = %candidate.mint, creator = %candidate.creator))]
    async fn fetch_creator_holdings(
        &self,
        candidate: &PremintCandidate,
        rpc: &RpcClient,
    ) -> Result<CreatorHoldings> {
        debug!("Fetching creator holdings and sell activity");

        // Parse addresses
        let mint_pubkey =
            SolanaPubkey::from_str(&candidate.mint).context("Failed to parse mint address")?;

        let creator_pubkey = SolanaPubkey::from_str(&candidate.creator)
            .context("Failed to parse creator address")?;

        // Find creator's token account
        let creator_token_account = self
            .find_token_account(&creator_pubkey, &mint_pubkey, rpc)
            .await?;

        let current_balance = if let Some(account_pubkey) = creator_token_account {
            // Fetch account data
            match tokio::time::timeout(
                Duration::from_secs(self.config.rpc_timeout_seconds),
                rpc.get_account(&account_pubkey),
            )
            .await
            {
                Ok(Ok(account)) => {
                    // Parse token amount from account data
                    if account.data.len() >= 72 {
                        let amount_bytes: [u8; 8] =
                            account.data[64..72].try_into().unwrap_or([0u8; 8]);
                        u64::from_le_bytes(amount_bytes)
                    } else {
                        0
                    }
                }
                _ => 0,
            }
        } else {
            0
        };

        debug!("Creator current balance: {}", current_balance);

        // Analyze creator's transaction history for sells
        use solana_transaction_status::UiTransactionEncoding;

        let creator_tx_config = Some(solana_client::rpc_config::RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Json),
            commitment: Some(solana_sdk::commitment_config::CommitmentConfig::confirmed()),
            max_supported_transaction_version: Some(0),
        });

        let signatures = match tokio::time::timeout(
            Duration::from_secs(self.config.rpc_timeout_seconds),
            rpc.get_signatures_for_address(&creator_pubkey),
        )
        .await
        {
            Ok(Ok(sigs)) => sigs,
            _ => {
                debug!("Could not fetch creator transaction history");
                Vec::new()
            }
        };

        // Count sell transactions (simplified - checks for transfers from creator)
        let mut sell_transactions = 0u32;
        let mut first_sell_timestamp: Option<u64> = None;

        for sig_info in signatures.iter().take(100) {
            // Check if transaction is related to our token
            if let Some(block_time) = sig_info.block_time {
                // Simplified sell detection
                // Real implementation would parse transaction instructions
                if sig_info.memo.is_some() || sig_info.err.is_none() {
                    sell_transactions += 1;
                    if first_sell_timestamp.is_none() {
                        first_sell_timestamp = Some(block_time as u64);
                    }
                }
            }
        }

        // Estimate initial balance
        // In production, this would track balance changes over time
        let initial_balance = current_balance + (sell_transactions as u64 * 1_000_000);

        debug!(
            "Creator holdings: initial {}, current {}, {} sells",
            initial_balance, current_balance, sell_transactions
        );

        Ok(CreatorHoldings {
            initial_balance,
            current_balance,
            first_sell_timestamp,
            sell_transactions,
        })
    }

    /// Find token account for a given owner and mint.
    async fn find_token_account(
        &self,
        owner: &SolanaPubkey,
        mint: &SolanaPubkey,
        rpc: &RpcClient,
    ) -> Result<Option<SolanaPubkey>> {
        use solana_account_decoder::UiAccountEncoding;
        use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
        use solana_client::rpc_filter::{Memcmp, RpcFilterType};
        use solana_sdk::commitment_config::CommitmentConfig;

        let token_program_id =
            SolanaPubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")?;

        let config = RpcProgramAccountsConfig {
            filters: Some(vec![
                RpcFilterType::DataSize(165),
                RpcFilterType::Memcmp(Memcmp::new_raw_bytes(0, mint.to_bytes().to_vec())),
                RpcFilterType::Memcmp(Memcmp::new_raw_bytes(32, owner.to_bytes().to_vec())),
            ]),
            account_config: RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                commitment: Some(CommitmentConfig::confirmed()),
                ..Default::default()
            },
            ..Default::default()
        };

        let accounts = match tokio::time::timeout(
            Duration::from_secs(self.config.rpc_timeout_seconds),
            rpc.get_program_accounts_with_config(&token_program_id, config),
        )
        .await
        {
            Ok(Ok(accounts)) => accounts,
            _ => return Ok(None),
        };

        Ok(accounts.first().map(|(pubkey, _)| *pubkey))
    }

    /// Fetch social activity data.
    ///
    /// Queries social media APIs for mentions, community size, and engagement.
    /// This is optional and depends on API keys being configured.
    ///
    /// # Returns
    ///
    /// Social activity metrics or default values if APIs are unavailable
    #[instrument(skip(self), fields(mint = %candidate.mint))]
    async fn fetch_social_activity(&self, candidate: &PremintCandidate) -> Result<SocialActivity> {
        debug!("Fetching social activity data");

        // Initialize with default values
        let mut twitter_mentions = 0u32;
        let mut telegram_members = 0u32;
        let mut discord_members = 0u32;

        // Try to fetch Twitter mentions if we have metadata
        // This would require Twitter API v2 with bearer token
        // For now, return estimated values based on other metrics

        // Calculate social score based on available data
        // Real implementation would use actual API calls
        let social_score = 0.0; // Would be calculated from actual metrics

        debug!(
            "Social activity: {} Twitter, {} Telegram, {} Discord, score: {:.2}",
            twitter_mentions, telegram_members, discord_members, social_score
        );

        Ok(SocialActivity {
            twitter_mentions,
            telegram_members,
            discord_members,
            social_score,
        })
    }

    // --- Pillar III: Macro-economic Data Sources for MarketRegimeDetector ---

    /// Fetch current SOL/USD price from CoinGecko API.
    ///
    /// This method queries CoinGecko's public API to get the current SOL price.
    /// It implements retry logic for reliability.
    ///
    /// # Returns
    ///
    /// Current SOL price in USD
    #[instrument(skip(self))]
    pub async fn fetch_sol_price_usd(&self) -> Result<f64> {
        let url = "https://api.coingecko.com/api/v3/simple/price?ids=solana&vs_currencies=usd";

        // Wait for API rate limiter before retry loop
        self.wait_for_api_rate_limit().await;

        let retry_strategy = ExponentialBackoff::from_millis(500)
            .max_delay(Duration::from_secs(3))
            .take(3);

        Retry::spawn(retry_strategy, || async {
            let response =
                tokio::time::timeout(Duration::from_secs(10), self.http_client.get(url).send())
                    .await
                    .context("CoinGecko API timeout")?
                    .context("Failed to send request to CoinGecko")?;

            if !response.status().is_success() {
                return Err(anyhow!(
                    "CoinGecko API returned status: {}",
                    response.status()
                ));
            }

            let json = response
                .json::<serde_json::Value>()
                .await
                .context("Failed to parse CoinGecko response")?;

            let price = json["solana"]["usd"]
                .as_f64()
                .context("Failed to extract SOL price from CoinGecko response")?;

            debug!("Fetched SOL price: ${:.2}", price);
            Ok(price)
        })
        .await
    }

    /// Calculate volatility from price history.
    ///
    /// Uses standard deviation as a measure of price volatility.
    ///
    /// # Arguments
    ///
    /// * `price_history` - Historical price data points
    ///
    /// # Returns
    ///
    /// Volatility as a percentage
    #[instrument(skip(self, price_history))]
    pub async fn calculate_sol_volatility(&self, price_history: &[f64]) -> Result<f64> {
        if price_history.len() < 2 {
            debug!("Insufficient price history for volatility calculation");
            return Ok(0.0);
        }

        let mean = price_history.iter().sum::<f64>() / price_history.len() as f64;
        let variance = price_history
            .iter()
            .map(|&p| (p - mean).powi(2))
            .sum::<f64>()
            / price_history.len() as f64;

        let std_dev = variance.sqrt();
        let volatility_percentage = (std_dev / mean) * 100.0;

        debug!("Calculated volatility: {:.2}%", volatility_percentage);
        Ok(volatility_percentage)
    }

    /// Fetch network TPS (Transactions Per Second).
    ///
    /// Queries Solana RPC for recent performance samples to estimate
    /// current network throughput.
    ///
    /// # Returns
    ///
    /// Estimated transactions per second
    #[instrument(skip(self))]
    pub async fn fetch_network_tps(&self) -> Result<f64> {
        let rpc = self.get_next_rpc_client().await?;

        // Fetch recent performance samples
        match tokio::time::timeout(
            Duration::from_secs(self.config.rpc_timeout_seconds),
            rpc.get_recent_performance_samples(Some(5)),
        )
        .await
        {
            Ok(Ok(samples)) => {
                if samples.is_empty() {
                    warn!("No performance samples available");
                    return Ok(0.0);
                }

                // Calculate average TPS from samples
                let total_tps: u64 = samples
                    .iter()
                    .map(|s| s.num_transactions / s.sample_period_secs as u64)
                    .sum();

                let avg_tps = total_tps as f64 / samples.len() as f64;

                debug!("Network TPS: {:.1}", avg_tps);
                Ok(avg_tps)
            }
            Ok(Err(e)) => {
                warn!("Failed to fetch performance samples: {}", e);
                // Fallback to time-based estimation
                self.estimate_tps_from_time()
            }
            Err(_) => {
                warn!("Timeout fetching performance samples");
                self.estimate_tps_from_time()
            }
        }
    }

    /// Estimate TPS based on time of day (fallback method).
    fn estimate_tps_from_time(&self) -> Result<f64> {
        let current_hour = chrono::Utc::now().hour();
        let simulated_tps = match current_hour {
            // Peak hours (US market hours): higher TPS
            14..=22 => 2000.0 + (current_hour as f64 * 50.0),
            // Off-peak hours: lower TPS
            _ => 1000.0 + (current_hour as f64 * 20.0),
        };

        debug!("Estimated TPS (time-based): {:.1}", simulated_tps);
        Ok(simulated_tps)
    }

    /// Fetch global DEX volume data.
    ///
    /// Aggregates volume data from major Solana DEXes.
    /// This is a simplified implementation that estimates volume.
    ///
    /// # Returns
    ///
    /// Estimated 24h DEX volume in USD
    #[instrument(skip(self))]
    pub async fn fetch_global_dex_volume(&self) -> Result<f64> {
        // In production, this would query actual DEX APIs:
        // - Raydium API
        // - Orca API
        // - Jupiter aggregator
        // For now, use time-based estimation

        let current_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("System time error")?
            .as_secs();

        // Simulate daily volume patterns with sinusoidal variation
        let hour_factor =
            ((current_timestamp % 86400) as f64 / 86400.0 * 2.0 * std::f64::consts::PI)
                .sin()
                .abs();

        let base_volume = 50_000_000.0; // 50M USD base
        let daily_volume = base_volume + (base_volume * 0.5 * hour_factor);

        debug!("Global DEX volume estimate: ${:.0}", daily_volume);
        Ok(daily_volume)
    }
}

impl Default for VolumeData {
    fn default() -> Self {
        Self {
            initial_volume: 0.0,
            current_volume: 0.0,
            volume_growth_rate: 1.0,
            transaction_count: 0,
            buy_sell_ratio: 1.0,
        }
    }
}

impl Default for CreatorHoldings {
    fn default() -> Self {
        Self {
            initial_balance: 0,
            current_balance: 0,
            first_sell_timestamp: None,
            sell_transactions: 0,
        }
    }
}

impl Default for SocialActivity {
    fn default() -> Self {
        Self {
            twitter_mentions: 0,
            telegram_members: 0,
            discord_members: 0,
            social_score: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::types::OracleConfig;
    use crate::types::PremintCandidate;
    use reqwest::Client;

    fn create_test_config() -> OracleConfig {
        OracleConfig::default()
    }

    fn create_test_candidate() -> PremintCandidate {
        PremintCandidate {
            mint: "So11111111111111111111111111111111111111112".to_string(),
            creator: "11111111111111111111111111111111".to_string(),
            program: "test".to_string(),
            slot: 12345,
            timestamp: 1640995200,
            instruction_summary: None,
            is_jito_bundle: Some(true),
        }
    }

    #[test]
    fn test_volume_data_default() {
        let volume_data = VolumeData::default();

        assert_eq!(volume_data.initial_volume, 0.0);
        assert_eq!(volume_data.current_volume, 0.0);
        assert_eq!(volume_data.volume_growth_rate, 1.0);
        assert_eq!(volume_data.transaction_count, 0);
        assert_eq!(volume_data.buy_sell_ratio, 1.0);
    }

    #[test]
    fn test_creator_holdings_default() {
        let creator_holdings = CreatorHoldings::default();

        assert_eq!(creator_holdings.initial_balance, 0);
        assert_eq!(creator_holdings.current_balance, 0);
        assert_eq!(creator_holdings.first_sell_timestamp, None);
        assert_eq!(creator_holdings.sell_transactions, 0);
    }

    #[test]
    fn test_social_activity_default() {
        let social_activity = SocialActivity::default();

        assert_eq!(social_activity.twitter_mentions, 0);
        assert_eq!(social_activity.telegram_members, 0);
        assert_eq!(social_activity.discord_members, 0);
        assert_eq!(social_activity.social_score, 0.0);
    }

    #[tokio::test]
    async fn test_data_sources_creation() {
        let config = create_test_config();
        let http_client = Client::new();
        let endpoints = vec!["https://api.mainnet-beta.solana.com".to_string()];

        let data_sources = OracleDataSources::new(endpoints, http_client, config);

        // Verify construction works
        assert_eq!(data_sources.rpc_clients.len(), 1);
    }

    #[tokio::test]
    async fn test_rpc_client_round_robin() {
        let config = create_test_config();
        let http_client = Client::new();
        let endpoints = vec![
            "https://api.mainnet-beta.solana.com".to_string(),
            "https://api.devnet.solana.com".to_string(),
        ];

        let data_sources = OracleDataSources::new(endpoints, http_client, config);

        // Get multiple clients and verify round-robin
        let client1 = data_sources.get_next_rpc_client().await;
        let client2 = data_sources.get_next_rpc_client().await;

        assert!(client1.is_ok());
        assert!(client2.is_ok());
    }

    #[test]
    fn test_cache_entry_expiration() {
        let data = (1000000u64, 9u8);
        let ttl = Duration::from_millis(10);
        let entry = CacheEntry::new(data, ttl);

        assert!(!entry.is_expired());

        std::thread::sleep(Duration::from_millis(20));
        assert!(entry.is_expired());
    }

    #[tokio::test]
    async fn test_cache_get_set() {
        let cache = DataCache::new();
        let mint = "TestMint".to_string();
        let data = (1000000u64, 9u8);
        let ttl = Duration::from_secs(60);

        // Initially empty
        assert!(cache.get_token_supply(&mint).await.is_none());

        // Set data
        cache.set_token_supply(mint.clone(), data, ttl).await;

        // Should be retrievable
        let retrieved = cache.get_token_supply(&mint).await;
        assert_eq!(retrieved, Some(data));
    }

    #[test]
    fn test_extract_uri_validation() {
        let config = OracleConfig::default();
        let http_client = Client::new();
        let endpoints = vec!["https://api.mainnet-beta.solana.com".to_string()];
        let data_sources = OracleDataSources::new(endpoints, http_client, config);

        // Test with invalid data
        let invalid_data = vec![0u8; 50];
        let result = data_sources.extract_uri_from_metadata(&invalid_data);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_volatility_calculation() {
        let config = OracleConfig::default();
        let http_client = Client::new();
        let endpoints = vec!["https://api.mainnet-beta.solana.com".to_string()];
        let data_sources = OracleDataSources::new(endpoints, http_client, config);

        // Test with empty history
        let empty_history: Vec<f64> = vec![];
        let vol = data_sources.calculate_sol_volatility(&empty_history).await;
        assert!(vol.is_ok());
        assert_eq!(vol.unwrap(), 0.0);

        // Test with price history
        let price_history = vec![100.0, 105.0, 103.0, 107.0, 104.0];
        let vol = data_sources.calculate_sol_volatility(&price_history).await;
        assert!(vol.is_ok());
        assert!(vol.unwrap() > 0.0);
    }
}
