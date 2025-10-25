//! Smart Money Tracker Module
//!
//! Real-time monitoring of smart wallet transactions on Solana for early pump detection.
//! Integrates with Nansen, Birdeye, and custom aggregators to track high-value wallet activity.
//!
//! # Key Features
//! - Multi-source smart wallet tracking (Nansen, Birdeye, custom)
//! - Real-time transaction monitoring
//! - <5s alert latency from transaction to detection
//! - Detection of >2 smart wallet purchases within 60s of token deploy
//! - Comprehensive logging: wallet, amount, time, token, tx
//!
//! # Integration
//! Works with EarlyPumpDetector and 100-Second Decision Engine

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{debug, error, info, instrument, warn};

/// Configuration for smart money tracking
#[derive(Debug, Clone)]
pub struct SmartMoneyConfig {
    /// Nansen API key (if available)
    pub nansen_api_key: Option<String>,
    /// Birdeye API key (fallback)
    pub birdeye_api_key: Option<String>,
    /// Custom smart wallet list
    pub custom_smart_wallets: Vec<String>,
    /// Time window for smart wallet detection (seconds)
    pub detection_window_secs: u64,
    /// Minimum number of smart wallets for alert
    pub min_smart_wallets: usize,
    /// Cache duration for smart wallet data (seconds)
    pub cache_duration_secs: u64,
    /// API request timeout (seconds)
    pub api_timeout_secs: u64,
    /// Enable Nansen integration
    pub enable_nansen: bool,
    /// Enable Birdeye integration
    pub enable_birdeye: bool,
}

impl Default for SmartMoneyConfig {
    fn default() -> Self {
        Self {
            nansen_api_key: None,
            birdeye_api_key: None,
            custom_smart_wallets: vec![],
            detection_window_secs: 60,
            min_smart_wallets: 2,
            cache_duration_secs: 300,
            api_timeout_secs: 5,
            enable_nansen: false,
            enable_birdeye: true,
        }
    }
}

/// Smart wallet transaction record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartWalletTransaction {
    /// Wallet address
    pub wallet: String,
    /// Token mint address
    pub token_mint: String,
    /// Transaction signature
    pub signature: String,
    /// Amount in base units
    pub amount: u64,
    /// Amount in SOL equivalent
    pub amount_sol: f64,
    /// Timestamp (unix seconds)
    pub timestamp: u64,
    /// Transaction type (buy/sell)
    pub tx_type: TransactionType,
    /// Source of detection
    pub source: DataSource,
}

/// Transaction type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransactionType {
    Buy,
    Sell,
}

/// Data source for smart wallet information
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DataSource {
    Nansen,
    Birdeye,
    Custom,
    OnChain,
}

/// Smart money alert trigger
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartMoneyAlert {
    /// Token mint that triggered alert
    pub token_mint: String,
    /// Token deployment timestamp
    pub deploy_timestamp: u64,
    /// Alert generation timestamp
    pub alert_timestamp: u64,
    /// Smart wallet transactions that triggered alert
    pub transactions: Vec<SmartWalletTransaction>,
    /// Number of unique smart wallets
    pub unique_wallets: usize,
    /// Total buy volume in SOL
    pub total_volume_sol: f64,
    /// Time from deploy to first smart wallet buy (seconds)
    pub time_to_first_buy_secs: u64,
}

/// Smart wallet profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartWalletProfile {
    /// Wallet address
    pub address: String,
    /// Source where wallet is marked as smart money
    pub source: DataSource,
    /// Success rate (0-100)
    pub success_rate: Option<f64>,
    /// Total profit in SOL
    pub total_profit_sol: Option<f64>,
    /// Number of successful trades
    pub successful_trades: Option<u32>,
    /// Last activity timestamp
    pub last_activity: u64,
}

/// Cache entry for smart wallet data
#[derive(Debug, Clone)]
struct CacheEntry<T> {
    data: T,
    expires_at: Instant,
}

/// Smart Money Tracker
pub struct SmartMoneyTracker {
    config: SmartMoneyConfig,
    http_client: Client,
    /// Cache of known smart wallets
    smart_wallets: Arc<RwLock<HashMap<String, SmartWalletProfile>>>,
    /// Recent transactions by token
    recent_transactions: Arc<RwLock<HashMap<String, VecDeque<SmartWalletTransaction>>>>,
    /// Cache for Nansen API responses
    nansen_cache: Arc<RwLock<HashMap<String, CacheEntry<Vec<SmartWalletTransaction>>>>>,
    /// Cache for Birdeye API responses
    birdeye_cache: Arc<RwLock<HashMap<String, CacheEntry<Vec<SmartWalletTransaction>>>>>,
}

impl SmartMoneyTracker {
    /// Create a new smart money tracker
    pub fn new(config: SmartMoneyConfig) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(config.api_timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        let mut smart_wallets = HashMap::new();
        
        // Initialize custom smart wallets
        for wallet in &config.custom_smart_wallets {
            smart_wallets.insert(
                wallet.clone(),
                SmartWalletProfile {
                    address: wallet.clone(),
                    source: DataSource::Custom,
                    success_rate: None,
                    total_profit_sol: None,
                    successful_trades: None,
                    last_activity: 0,
                },
            );
        }

        info!(
            "Initialized SmartMoneyTracker with {} custom wallets, nansen={}, birdeye={}",
            config.custom_smart_wallets.len(),
            config.enable_nansen,
            config.enable_birdeye
        );

        Self {
            config,
            http_client,
            smart_wallets: Arc::new(RwLock::new(smart_wallets)),
            recent_transactions: Arc::new(RwLock::new(HashMap::new())),
            nansen_cache: Arc::new(RwLock::new(HashMap::new())),
            birdeye_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check smart money involvement for a token
    ///
    /// Returns (score, unique_wallets, transactions)
    /// Score: 0-100 based on smart wallet activity
    #[instrument(skip(self))]
    pub async fn check_smart_money(
        &self,
        token_mint: &str,
        deploy_timestamp: u64,
    ) -> Result<(u8, usize, Vec<SmartWalletTransaction>)> {
        let start = Instant::now();
        debug!("Checking smart money for token: {}", token_mint);

        // Fetch transactions from all available sources
        let mut all_transactions = Vec::new();

        // Try Nansen first if enabled
        if self.config.enable_nansen && self.config.nansen_api_key.is_some() {
            match self.fetch_nansen_transactions(token_mint).await {
                Ok(txs) => {
                    debug!("Fetched {} transactions from Nansen", txs.len());
                    all_transactions.extend(txs);
                }
                Err(e) => {
                    warn!("Failed to fetch from Nansen: {}", e);
                }
            }
        }

        // Try Birdeye as fallback/supplement
        if self.config.enable_birdeye && self.config.birdeye_api_key.is_some() {
            match self.fetch_birdeye_transactions(token_mint).await {
                Ok(txs) => {
                    debug!("Fetched {} transactions from Birdeye", txs.len());
                    all_transactions.extend(txs);
                }
                Err(e) => {
                    warn!("Failed to fetch from Birdeye: {}", e);
                }
            }
        }

        // Check recent transactions cache
        let cached_txs = self.get_cached_transactions(token_mint).await;
        all_transactions.extend(cached_txs);

        // Filter to only BUY transactions within detection window
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let detection_end = deploy_timestamp + self.config.detection_window_secs;
        
        let relevant_transactions: Vec<_> = all_transactions
            .into_iter()
            .filter(|tx| {
                tx.tx_type == TransactionType::Buy
                    && tx.timestamp >= deploy_timestamp
                    && tx.timestamp <= detection_end
            })
            .collect();

        // Count unique smart wallets
        let mut unique_wallets = std::collections::HashSet::new();
        for tx in &relevant_transactions {
            unique_wallets.insert(tx.wallet.clone());
        }

        let unique_wallet_count = unique_wallets.len();

        // Calculate score based on smart wallet activity
        let score = self.calculate_smart_money_score(
            unique_wallet_count,
            &relevant_transactions,
            deploy_timestamp,
        );

        // Cache the transactions
        self.cache_transactions(token_mint, relevant_transactions.clone())
            .await;

        let elapsed = start.elapsed().as_millis();
        debug!(
            "Smart money check completed in {}ms: score={}, wallets={}, transactions={}",
            elapsed,
            score,
            unique_wallet_count,
            relevant_transactions.len()
        );

        Ok((score, unique_wallet_count, relevant_transactions))
    }

    /// Generate alert if smart money threshold is met
    #[instrument(skip(self))]
    pub async fn check_alert(
        &self,
        token_mint: &str,
        deploy_timestamp: u64,
    ) -> Result<Option<SmartMoneyAlert>> {
        let (score, unique_wallets, transactions) =
            self.check_smart_money(token_mint, deploy_timestamp).await?;

        if unique_wallets >= self.config.min_smart_wallets {
            let total_volume_sol: f64 = transactions.iter().map(|tx| tx.amount_sol).sum();
            
            let time_to_first_buy_secs = transactions
                .iter()
                .map(|tx| tx.timestamp.saturating_sub(deploy_timestamp))
                .min()
                .unwrap_or(0);

            let alert = SmartMoneyAlert {
                token_mint: token_mint.to_string(),
                deploy_timestamp,
                alert_timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                transactions,
                unique_wallets,
                total_volume_sol,
                time_to_first_buy_secs,
            };

            info!(
                "🚨 SMART MONEY ALERT: {} unique wallets bought {} within {}s of deploy!",
                alert.unique_wallets, token_mint, alert.time_to_first_buy_secs
            );

            Ok(Some(alert))
        } else {
            debug!(
                "No alert: only {} smart wallets (need {})",
                unique_wallets, self.config.min_smart_wallets
            );
            Ok(None)
        }
    }

    /// Fetch transactions from Nansen API
    async fn fetch_nansen_transactions(
        &self,
        token_mint: &str,
    ) -> Result<Vec<SmartWalletTransaction>> {
        // Check cache first
        if let Some(cached) = self.check_nansen_cache(token_mint).await {
            return Ok(cached);
        }

        let api_key = self
            .config
            .nansen_api_key
            .as_ref()
            .ok_or_else(|| anyhow!("Nansen API key not configured"))?;

        // Nansen API endpoint (placeholder - actual endpoint depends on Nansen's API)
        let url = format!(
            "https://api.nansen.ai/v1/token/{}/smart-money",
            token_mint
        );

        let response = self
            .http_client
            .get(&url)
            .header("X-API-KEY", api_key)
            .send()
            .await
            .context("Failed to fetch from Nansen API")?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Nansen API request failed: {}",
                response.status()
            ));
        }

        let data: NansenResponse = response
            .json()
            .await
            .context("Failed to parse Nansen response")?;

        let transactions = data.to_transactions(token_mint);

        // Cache the result
        self.cache_nansen_response(token_mint, transactions.clone())
            .await;

        Ok(transactions)
    }

    /// Fetch transactions from Birdeye API
    async fn fetch_birdeye_transactions(
        &self,
        token_mint: &str,
    ) -> Result<Vec<SmartWalletTransaction>> {
        // Check cache first
        if let Some(cached) = self.check_birdeye_cache(token_mint).await {
            return Ok(cached);
        }

        let api_key = self
            .config
            .birdeye_api_key
            .as_ref()
            .ok_or_else(|| anyhow!("Birdeye API key not configured"))?;

        // Birdeye API endpoint for token trades
        let url = format!(
            "https://public-api.birdeye.so/defi/txs/token?address={}",
            token_mint
        );

        let response = self
            .http_client
            .get(&url)
            .header("X-API-KEY", api_key)
            .header("x-chain", "solana")
            .send()
            .await
            .context("Failed to fetch from Birdeye API")?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Birdeye API request failed: {}",
                response.status()
            ));
        }

        let data: BirdeyeResponse = response
            .json()
            .await
            .context("Failed to parse Birdeye response")?;

        let transactions = self.filter_smart_wallets(data.to_transactions(token_mint)).await;

        // Cache the result
        self.cache_birdeye_response(token_mint, transactions.clone())
            .await;

        Ok(transactions)
    }

    /// Filter transactions to only include known smart wallets
    async fn filter_smart_wallets(
        &self,
        transactions: Vec<SmartWalletTransaction>,
    ) -> Vec<SmartWalletTransaction> {
        let smart_wallets = self.smart_wallets.read().await;
        transactions
            .into_iter()
            .filter(|tx| smart_wallets.contains_key(&tx.wallet))
            .collect()
    }

    /// Calculate smart money score (0-100)
    fn calculate_smart_money_score(
        &self,
        unique_wallets: usize,
        transactions: &[SmartWalletTransaction],
        deploy_timestamp: u64,
    ) -> u8 {
        if transactions.is_empty() {
            return 0;
        }

        let mut score = 0.0;

        // Factor 1: Number of unique smart wallets (40 points max)
        let wallet_score = (unique_wallets as f64 * 10.0).min(40.0);
        score += wallet_score;

        // Factor 2: Total volume (30 points max)
        let total_volume: f64 = transactions.iter().map(|tx| tx.amount_sol).sum();
        let volume_score = (total_volume / 10.0).min(30.0); // 1 point per 0.33 SOL
        score += volume_score;

        // Factor 3: Speed to market (30 points max)
        let avg_latency = transactions
            .iter()
            .map(|tx| tx.timestamp.saturating_sub(deploy_timestamp))
            .sum::<u64>() as f64
            / transactions.len() as f64;

        let speed_score = if avg_latency < 10.0 {
            30.0
        } else if avg_latency < 30.0 {
            20.0
        } else if avg_latency < 60.0 {
            10.0
        } else {
            0.0
        };
        score += speed_score;

        score.min(100.0) as u8
    }

    /// Get cached transactions for a token
    async fn get_cached_transactions(&self, token_mint: &str) -> Vec<SmartWalletTransaction> {
        let cache = self.recent_transactions.read().await;
        cache
            .get(token_mint)
            .map(|txs| txs.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Cache transactions for a token
    async fn cache_transactions(&self, token_mint: &str, transactions: Vec<SmartWalletTransaction>) {
        let mut cache = self.recent_transactions.write().await;
        let entry = cache.entry(token_mint.to_string()).or_insert_with(VecDeque::new);
        
        for tx in transactions {
            entry.push_back(tx);
        }

        // Keep only last 100 transactions per token
        while entry.len() > 100 {
            entry.pop_front();
        }
    }

    /// Check Nansen cache
    async fn check_nansen_cache(&self, token_mint: &str) -> Option<Vec<SmartWalletTransaction>> {
        let cache = self.nansen_cache.read().await;
        if let Some(entry) = cache.get(token_mint) {
            if entry.expires_at > Instant::now() {
                return Some(entry.data.clone());
            }
        }
        None
    }

    /// Cache Nansen response
    async fn cache_nansen_response(&self, token_mint: &str, data: Vec<SmartWalletTransaction>) {
        let mut cache = self.nansen_cache.write().await;
        cache.insert(
            token_mint.to_string(),
            CacheEntry {
                data,
                expires_at: Instant::now() + Duration::from_secs(self.config.cache_duration_secs),
            },
        );
    }

    /// Check Birdeye cache
    async fn check_birdeye_cache(&self, token_mint: &str) -> Option<Vec<SmartWalletTransaction>> {
        let cache = self.birdeye_cache.read().await;
        if let Some(entry) = cache.get(token_mint) {
            if entry.expires_at > Instant::now() {
                return Some(entry.data.clone());
            }
        }
        None
    }

    /// Cache Birdeye response
    async fn cache_birdeye_response(&self, token_mint: &str, data: Vec<SmartWalletTransaction>) {
        let mut cache = self.birdeye_cache.write().await;
        cache.insert(
            token_mint.to_string(),
            CacheEntry {
                data,
                expires_at: Instant::now() + Duration::from_secs(self.config.cache_duration_secs),
            },
        );
    }

    /// Add a smart wallet to the tracking list
    pub async fn add_smart_wallet(&self, profile: SmartWalletProfile) {
        let mut wallets = self.smart_wallets.write().await;
        wallets.insert(profile.address.clone(), profile);
    }

    /// Get all tracked smart wallets
    pub async fn get_smart_wallets(&self) -> Vec<SmartWalletProfile> {
        let wallets = self.smart_wallets.read().await;
        wallets.values().cloned().collect()
    }
}

// API Response types

#[derive(Debug, Deserialize)]
struct NansenResponse {
    transactions: Vec<NansenTransaction>,
}

#[derive(Debug, Deserialize)]
struct NansenTransaction {
    wallet: String,
    signature: String,
    amount: String,
    amount_usd: f64,
    timestamp: u64,
    #[serde(rename = "type")]
    tx_type: String,
}

impl NansenResponse {
    fn to_transactions(&self, token_mint: &str) -> Vec<SmartWalletTransaction> {
        self.transactions
            .iter()
            .map(|tx| SmartWalletTransaction {
                wallet: tx.wallet.clone(),
                token_mint: token_mint.to_string(),
                signature: tx.signature.clone(),
                amount: tx.amount.parse().unwrap_or(0),
                amount_sol: tx.amount_usd / 100.0, // Rough conversion
                timestamp: tx.timestamp,
                tx_type: if tx.tx_type.to_lowercase().contains("buy") {
                    TransactionType::Buy
                } else {
                    TransactionType::Sell
                },
                source: DataSource::Nansen,
            })
            .collect()
    }
}

#[derive(Debug, Deserialize)]
struct BirdeyeResponse {
    success: bool,
    data: BirdeyeData,
}

#[derive(Debug, Deserialize)]
struct BirdeyeData {
    items: Vec<BirdeyeTransaction>,
}

#[derive(Debug, Deserialize)]
struct BirdeyeTransaction {
    #[serde(rename = "owner")]
    wallet: String,
    #[serde(rename = "txHash")]
    signature: String,
    #[serde(rename = "blockUnixTime")]
    timestamp: u64,
    #[serde(rename = "side")]
    side: String,
    #[serde(rename = "amount")]
    amount: f64,
    #[serde(rename = "solAmount")]
    sol_amount: f64,
}

impl BirdeyeResponse {
    fn to_transactions(&self, token_mint: &str) -> Vec<SmartWalletTransaction> {
        self.data
            .items
            .iter()
            .map(|tx| SmartWalletTransaction {
                wallet: tx.wallet.clone(),
                token_mint: token_mint.to_string(),
                signature: tx.signature.clone(),
                amount: tx.amount as u64,
                amount_sol: tx.sol_amount,
                timestamp: tx.timestamp,
                tx_type: if tx.side.to_lowercase() == "buy" {
                    TransactionType::Buy
                } else {
                    TransactionType::Sell
                },
                source: DataSource::Birdeye,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smart_money_config_default() {
        let config = SmartMoneyConfig::default();
        assert_eq!(config.detection_window_secs, 60);
        assert_eq!(config.min_smart_wallets, 2);
        assert!(config.enable_birdeye);
    }

    #[test]
    fn test_calculate_score_no_transactions() {
        let config = SmartMoneyConfig::default();
        let tracker = SmartMoneyTracker::new(config);
        let score = tracker.calculate_smart_money_score(0, &[], 0);
        assert_eq!(score, 0);
    }

    #[test]
    fn test_calculate_score_with_transactions() {
        let config = SmartMoneyConfig::default();
        let tracker = SmartMoneyTracker::new(config);
        
        let transactions = vec![
            SmartWalletTransaction {
                wallet: "wallet1".to_string(),
                token_mint: "token1".to_string(),
                signature: "sig1".to_string(),
                amount: 1000000,
                amount_sol: 5.0,
                timestamp: 100,
                tx_type: TransactionType::Buy,
                source: DataSource::Custom,
            },
            SmartWalletTransaction {
                wallet: "wallet2".to_string(),
                token_mint: "token1".to_string(),
                signature: "sig2".to_string(),
                amount: 2000000,
                amount_sol: 10.0,
                timestamp: 110,
                tx_type: TransactionType::Buy,
                source: DataSource::Custom,
            },
        ];

        let score = tracker.calculate_smart_money_score(2, &transactions, 95);
        assert!(score > 0);
        assert!(score <= 100);
    }
}
