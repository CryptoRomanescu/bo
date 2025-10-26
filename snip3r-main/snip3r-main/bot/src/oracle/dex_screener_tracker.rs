//! DEX Screener Trending Tracker Module
//!
//! Real-time monitoring of DEX Screener trending and ranking data for Solana tokens.
//! Tracks trending positions, velocity, momentum, and correlates with price/volume changes.
//!
//! # Key Features
//! - Real-time trending position tracking
//! - Entry/exit event detection with <4s latency
//! - Trending velocity and momentum calculation
//! - Pump/dump correlation analysis
//! - Multi-metric trending score integration
//!
//! # Integration
//! Works with Oracle scoring system to provide trending-based signals for memecoin launches.

use anyhow::{anyhow, Context, Result};
use governor::{Quota, RateLimiter};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{debug, error, info, instrument, warn};

use super::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};

/// Configuration for DEX Screener trending tracking
#[derive(Debug, Clone)]
pub struct DexScreenerConfig {
    /// DEX Screener API key (optional, for higher rate limits)
    pub api_key: Option<String>,
    /// Base URL for DEX Screener API
    pub base_url: String,
    /// Polling interval for trending data (seconds)
    pub polling_interval_secs: u64,
    /// Cache duration for trending data (seconds)
    pub cache_duration_secs: u64,
    /// API request timeout (seconds)
    pub api_timeout_secs: u64,
    /// Minimum trending rank to trigger events (1-100)
    pub min_trending_rank: u32,
    /// Time window for velocity calculation (seconds)
    pub velocity_window_secs: u64,
    /// Enable trending tracking
    pub enable_tracking: bool,
    /// Maximum history size per token (positions + events)
    pub max_history_size: usize,
    /// Rate limit: requests per minute
    pub rate_limit_per_minute: u32,
    /// Enable circuit breaker for API failures
    pub enable_circuit_breaker: bool,
    /// Circuit breaker failure threshold
    pub circuit_breaker_failure_threshold: u32,
    /// Circuit breaker cooldown duration (seconds)
    pub circuit_breaker_cooldown_secs: u64,
    /// Maximum retries on API failure
    pub max_retries: u32,
    /// Use stale cache data on API failure
    pub fallback_to_stale_cache: bool,
    /// Maximum age of stale cache data to use (seconds)
    pub max_stale_cache_age_secs: u64,
}

impl Default for DexScreenerConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: "https://api.dexscreener.com/latest".to_string(),
            polling_interval_secs: 3, // <4s latency requirement
            cache_duration_secs: 2,
            api_timeout_secs: 2,
            min_trending_rank: 100,
            velocity_window_secs: 300, // 5 minutes for velocity
            enable_tracking: true,
            max_history_size: 100,
            rate_limit_per_minute: 20, // Conservative default
            enable_circuit_breaker: true,
            circuit_breaker_failure_threshold: 5,
            circuit_breaker_cooldown_secs: 60,
            max_retries: 3,
            fallback_to_stale_cache: true,
            max_stale_cache_age_secs: 60,
        }
    }
}

/// Trending position and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendingPosition {
    /// Token pair address
    pub pair_address: String,
    /// Token mint address
    pub token_address: String,
    /// Current trending rank (1 = top trending)
    pub rank: u32,
    /// Previous rank (for velocity calculation)
    pub previous_rank: Option<u32>,
    /// Timestamp when entered trending
    pub entry_timestamp: u64,
    /// Timestamp of current position
    pub timestamp: u64,
    /// Price at entry
    pub entry_price: f64,
    /// Current price
    pub current_price: f64,
    /// Volume at entry (24h USD)
    pub entry_volume: f64,
    /// Current volume (24h USD)
    pub current_volume: f64,
    /// Liquidity in USD
    pub liquidity_usd: f64,
    /// Market cap in USD
    pub market_cap_usd: Option<f64>,
}

/// Trending event types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TrendingEvent {
    /// Token entered trending (rank)
    Entry { rank: u32 },
    /// Token exited trending
    Exit { final_rank: u32 },
    /// Significant rank improvement (old_rank -> new_rank)
    RankUp { old_rank: u32, new_rank: u32 },
    /// Significant rank decline (old_rank -> new_rank)
    RankDown { old_rank: u32, new_rank: u32 },
    /// High velocity momentum (rank change in time window)
    HighMomentum { velocity: f64 },
}

/// Trending event with full context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendingEventRecord {
    /// Token address
    pub token_address: String,
    /// Event type
    pub event: TrendingEvent,
    /// Current trending position
    pub position: TrendingPosition,
    /// Time in trending (seconds)
    pub duration_secs: u64,
    /// Velocity (rank change per minute)
    pub velocity: f64,
    /// Price change since entry (%)
    pub price_change_pct: f64,
    /// Volume change since entry (%)
    pub volume_change_pct: f64,
    /// Event timestamp
    pub timestamp: u64,
}

/// Trending metrics for scoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendingMetrics {
    /// Is token currently trending
    pub is_trending: bool,
    /// Current trending rank (None if not trending)
    pub current_rank: Option<u32>,
    /// Time in trending (seconds)
    pub duration_in_trending: u64,
    /// Trending velocity (rank change per minute)
    pub velocity: f64,
    /// Momentum score (0-100)
    pub momentum_score: f64,
    /// Price pump correlation (0-1)
    pub price_correlation: f64,
    /// Volume correlation (0-1)
    pub volume_correlation: f64,
    /// Recent trending events
    pub recent_events: Vec<TrendingEvent>,
}

impl Default for TrendingMetrics {
    fn default() -> Self {
        Self {
            is_trending: false,
            current_rank: None,
            duration_in_trending: 0,
            velocity: 0.0,
            momentum_score: 0.0,
            price_correlation: 0.0,
            volume_correlation: 0.0,
            recent_events: vec![],
        }
    }
}

/// DEX Screener API response for trending pairs
#[derive(Debug, Clone, Deserialize)]
struct DexScreenerTrendingResponse {
    pairs: Option<Vec<DexScreenerPair>>,
}

/// DEX Screener pair data
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DexScreenerPair {
    chain_id: String,
    pair_address: String,
    base_token: DexScreenerToken,
    quote_token: DexScreenerToken,
    price_native: String,
    price_usd: Option<String>,
    volume: DexScreenerVolume,
    liquidity: DexScreenerLiquidity,
    fdv: Option<f64>,
    market_cap: Option<f64>,
    price_change: DexScreenerPriceChange,
}

#[derive(Debug, Clone, Deserialize)]
struct DexScreenerToken {
    address: String,
    name: String,
    symbol: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DexScreenerVolume {
    h24: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct DexScreenerLiquidity {
    usd: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct DexScreenerPriceChange {
    h24: f64,
}

/// Cache entry for trending data
#[derive(Debug, Clone)]
struct CacheEntry<T> {
    data: T,
    expires_at: Instant,
}

/// Trending history for a token
#[derive(Debug, Clone)]
struct TrendingHistory {
    positions: VecDeque<TrendingPosition>,
    events: VecDeque<TrendingEventRecord>,
    max_history: usize,
}

impl TrendingHistory {
    fn new(max_history: usize) -> Self {
        Self {
            positions: VecDeque::new(),
            events: VecDeque::new(),
            max_history,
        }
    }

    fn add_position(&mut self, position: TrendingPosition) {
        self.positions.push_back(position);
        while self.positions.len() > self.max_history {
            self.positions.pop_front();
        }
    }

    fn add_event(&mut self, event: TrendingEventRecord) {
        self.events.push_back(event);
        while self.events.len() > self.max_history {
            self.events.pop_front();
        }
    }
}

/// DEX Screener Trending Tracker
pub struct DexScreenerTracker {
    config: DexScreenerConfig,
    http_client: Client,
    /// Current trending positions by token address
    trending_positions: Arc<RwLock<HashMap<String, TrendingPosition>>>,
    /// Historical trending data by token address
    trending_history: Arc<RwLock<HashMap<String, TrendingHistory>>>,
    /// Cache for API responses
    api_cache: Arc<RwLock<HashMap<String, CacheEntry<DexScreenerTrendingResponse>>>>,
    /// Last poll timestamp
    last_poll: Arc<RwLock<Option<Instant>>>,
    /// Rate limiter for API requests
    rate_limiter: Arc<RateLimiter<governor::state::direct::NotKeyed, governor::state::InMemoryState, governor::clock::DefaultClock>>,
    /// Circuit breaker for API endpoint
    circuit_breaker: Option<Arc<CircuitBreaker>>,
    /// Failure counter for circuit breaker
    consecutive_failures: Arc<RwLock<u32>>,
}

impl DexScreenerTracker {
    /// Create a new DEX Screener tracker
    pub fn new(config: DexScreenerConfig) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(config.api_timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        // Create rate limiter
        let rate_limit = NonZeroU32::new(config.rate_limit_per_minute)
            .expect("Rate limit must be non-zero");
        let quota = Quota::per_minute(rate_limit);
        let rate_limiter = Arc::new(RateLimiter::direct(quota));

        // Create circuit breaker if enabled
        let circuit_breaker = if config.enable_circuit_breaker {
            Some(Arc::new(CircuitBreaker::new(
                config.circuit_breaker_failure_threshold,
                config.circuit_breaker_cooldown_secs,
                50, // sample_size
            )))
        } else {
            None
        };

        info!(
            "Initialized DexScreenerTracker with polling_interval={}s, cache={}s, rate_limit={}/min, circuit_breaker={}",
            config.polling_interval_secs, 
            config.cache_duration_secs,
            config.rate_limit_per_minute,
            config.enable_circuit_breaker
        );

        Self {
            config,
            http_client,
            trending_positions: Arc::new(RwLock::new(HashMap::new())),
            trending_history: Arc::new(RwLock::new(HashMap::new())),
            api_cache: Arc::new(RwLock::new(HashMap::new())),
            last_poll: Arc::new(RwLock::new(None)),
            rate_limiter,
            circuit_breaker,
            consecutive_failures: Arc::new(RwLock::new(0)),
        }
    }

    /// Get trending metrics for a token
    #[instrument(skip(self), fields(token = %token_address))]
    pub async fn get_trending_metrics(&self, token_address: &str) -> Result<TrendingMetrics> {
        // Check if we need to poll for new data
        self.maybe_poll_trending().await?;

        let positions = self.trending_positions.read().await;
        let history = self.trending_history.read().await;

        let current_position = positions.get(token_address);
        
        match current_position {
            Some(pos) => {
                let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
                let duration = now.saturating_sub(pos.entry_timestamp);
                
                // Calculate velocity and momentum
                let velocity = self.calculate_velocity(token_address, &history).await;
                let momentum_score = self.calculate_momentum_score(pos, velocity);
                
                // Calculate correlations
                let price_correlation = self.calculate_price_correlation(token_address, &history).await;
                let volume_correlation = self.calculate_volume_correlation(token_address, &history).await;
                
                // Get recent events
                let recent_events = if let Some(hist) = history.get(token_address) {
                    hist.events.iter()
                        .rev()
                        .take(5)
                        .map(|e| e.event.clone())
                        .collect()
                } else {
                    vec![]
                };

                Ok(TrendingMetrics {
                    is_trending: true,
                    current_rank: Some(pos.rank),
                    duration_in_trending: duration,
                    velocity,
                    momentum_score,
                    price_correlation,
                    volume_correlation,
                    recent_events,
                })
            }
            None => Ok(TrendingMetrics::default()),
        }
    }

    /// Poll trending data from DEX Screener API
    async fn maybe_poll_trending(&self) -> Result<()> {
        if !self.config.enable_tracking {
            return Ok(());
        }

        let mut last_poll = self.last_poll.write().await;
        let should_poll = match *last_poll {
            None => true,
            Some(last) => last.elapsed() > Duration::from_secs(self.config.polling_interval_secs),
        };

        if should_poll {
            drop(last_poll); // Release lock before async operation
            self.poll_trending_data().await?;
            let mut last_poll = self.last_poll.write().await;
            *last_poll = Some(Instant::now());
        }

        Ok(())
    }

    /// Poll trending data from DEX Screener API
    #[instrument(skip(self))]
    async fn poll_trending_data(&self) -> Result<()> {
        debug!("Polling DEX Screener trending data");

        // Check circuit breaker state
        if let Some(ref cb) = self.circuit_breaker {
            if !cb.is_available("dex_screener_api").await {
                warn!("Circuit breaker open for DEX Screener API, using fallback");
                return self.use_fallback_data().await;
            }
        }

        // Wait for rate limit
        self.rate_limiter.until_ready().await;

        // Attempt to fetch with retries
        let result = self.fetch_with_retries().await;

        match result {
            Ok(trending_response) => {
                // Reset failure counter on success
                let mut failures = self.consecutive_failures.write().await;
                *failures = 0;
                drop(failures);

                // Record success in circuit breaker
                if let Some(ref cb) = self.circuit_breaker {
                    cb.record_success("dex_screener_api").await;
                }

                // Process trending pairs
                if let Some(pairs) = trending_response.pairs {
                    self.process_trending_pairs(pairs).await?;
                }

                debug!("Successfully polled trending data");
                Ok(())
            }
            Err(e) => {
                // Increment failure counter
                let mut failures = self.consecutive_failures.write().await;
                *failures += 1;
                let failure_count = *failures;
                drop(failures);

                // Record failure in circuit breaker
                if let Some(ref cb) = self.circuit_breaker {
                    cb.record_failure("dex_screener_api").await;
                }

                error!(
                    error = %e,
                    consecutive_failures = failure_count,
                    "Failed to poll DEX Screener API"
                );

                // Use fallback if configured
                if self.config.fallback_to_stale_cache {
                    warn!("Using stale cache data as fallback");
                    return self.use_fallback_data().await;
                }

                Err(e)
            }
        }
    }

    /// Fetch trending data with retries
    async fn fetch_with_retries(&self) -> Result<DexScreenerTrendingResponse> {
        let mut last_error = None;

        for attempt in 0..self.config.max_retries {
            if attempt > 0 {
                // Exponential backoff
                let delay = Duration::from_millis(100 * (1 << attempt));
                debug!(attempt, delay_ms = delay.as_millis(), "Retrying API request");
                tokio::time::sleep(delay).await;
            }

            match self.fetch_trending_data().await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    warn!(attempt, error = %e, "API request failed");
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("All retry attempts failed")))
    }

    /// Fetch trending data from DEX Screener API (single attempt)
    async fn fetch_trending_data(&self) -> Result<DexScreenerTrendingResponse> {
        // Fetch trending pairs for Solana
        let url = format!("{}/dex/search?q=solana", self.config.base_url);
        
        let mut request = self.http_client.get(&url);
        if let Some(api_key) = &self.config.api_key {
            request = request.header("X-API-KEY", api_key);
        }

        let response = request
            .send()
            .await
            .context("Failed to send request to DEX Screener")?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "DEX Screener API error: {}",
                response.status()
            ));
        }

        let trending_response: DexScreenerTrendingResponse = response
            .json()
            .await
            .context("Failed to parse trending response")?;

        Ok(trending_response)
    }

    /// Use fallback data from stale cache
    async fn use_fallback_data(&self) -> Result<()> {
        let cache = self.api_cache.read().await;
        
        if let Some(cached) = cache.get("trending_data") {
            let age = cached.expires_at.elapsed();
            if age < Duration::from_secs(self.config.max_stale_cache_age_secs) {
                info!(age_secs = age.as_secs(), "Using stale cache data");
                
                // Process cached data
                if let Some(pairs) = cached.data.pairs.clone() {
                    drop(cache); // Release read lock before processing
                    self.process_trending_pairs(pairs).await?;
                }
                
                return Ok(());
            } else {
                warn!(
                    age_secs = age.as_secs(),
                    max_age_secs = self.config.max_stale_cache_age_secs,
                    "Stale cache data too old, skipping"
                );
            }
        }

        Err(anyhow!("No fallback data available"))
    }

    /// Process trending pairs and detect events
    async fn process_trending_pairs(&self, pairs: Vec<DexScreenerPair>) -> Result<()> {
        let mut positions = self.trending_positions.write().await;
        let mut history = self.trending_history.write().await;
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        // Filter for Solana pairs only
        let solana_pairs: Vec<_> = pairs
            .into_iter()
            .filter(|p| p.chain_id == "solana")
            .enumerate()
            .collect();

        let mut current_tokens = HashMap::new();

        for (rank, pair) in solana_pairs {
            let token_address = pair.base_token.address.clone();
            let rank = (rank + 1) as u32;

            // Skip if rank is below threshold
            if rank > self.config.min_trending_rank {
                continue;
            }

            let price = pair.price_usd
                .as_ref()
                .and_then(|p| p.parse::<f64>().ok())
                .unwrap_or(0.0);

            let liquidity_usd = pair.liquidity.usd.unwrap_or(0.0);
            let volume = pair.volume.h24;

            // Check if token was already trending
            let previous_position = positions.get(&token_address);
            let previous_rank = previous_position.map(|p| p.rank);

            let position = if let Some(prev) = previous_position {
                // Update existing position
                TrendingPosition {
                    pair_address: pair.pair_address.clone(),
                    token_address: token_address.clone(),
                    rank,
                    previous_rank: Some(prev.rank),
                    entry_timestamp: prev.entry_timestamp,
                    timestamp: now,
                    entry_price: prev.entry_price,
                    current_price: price,
                    entry_volume: prev.entry_volume,
                    current_volume: volume,
                    liquidity_usd,
                    market_cap_usd: pair.market_cap,
                }
            } else {
                // New entry to trending
                TrendingPosition {
                    pair_address: pair.pair_address.clone(),
                    token_address: token_address.clone(),
                    rank,
                    previous_rank: None,
                    entry_timestamp: now,
                    timestamp: now,
                    entry_price: price,
                    current_price: price,
                    entry_volume: volume,
                    current_volume: volume,
                    liquidity_usd,
                    market_cap_usd: pair.market_cap,
                }
            };

            // Detect and emit events
            self.detect_and_emit_event(&token_address, &position, previous_position, &mut history).await;

            current_tokens.insert(token_address.clone(), position);
        }

        // Detect exit events for tokens no longer trending
        for (token_address, old_position) in positions.iter() {
            if !current_tokens.contains_key(token_address) {
                // Token exited trending
                let event = TrendingEvent::Exit {
                    final_rank: old_position.rank,
                };

                let duration = now.saturating_sub(old_position.entry_timestamp);
                let price_change_pct = ((old_position.current_price - old_position.entry_price) 
                    / old_position.entry_price) * 100.0;
                let volume_change_pct = ((old_position.current_volume - old_position.entry_volume) 
                    / old_position.entry_volume) * 100.0;

                let event_record = TrendingEventRecord {
                    token_address: token_address.clone(),
                    event,
                    position: old_position.clone(),
                    duration_secs: duration,
                    velocity: 0.0,
                    price_change_pct,
                    volume_change_pct,
                    timestamp: now,
                };

                info!(
                    "Trending EXIT: {} (rank={}, duration={}s, price_change={:.1}%)",
                    token_address, old_position.rank, duration, price_change_pct
                );

                history
                    .entry(token_address.clone())
                    .or_insert_with(|| TrendingHistory::new(self.config.max_history_size))
                    .add_event(event_record);
            }
        }

        // Update positions
        *positions = current_tokens;

        Ok(())
    }

    /// Detect and emit trending events
    async fn detect_and_emit_event(
        &self,
        token_address: &str,
        position: &TrendingPosition,
        previous_position: Option<&TrendingPosition>,
        history: &mut HashMap<String, TrendingHistory>,
    ) {
        let now = position.timestamp;
        let duration = now.saturating_sub(position.entry_timestamp);
        
        let velocity = if let Some(prev) = previous_position {
            let rank_change = prev.rank as i32 - position.rank as i32;
            let time_diff = (now - prev.timestamp) as f64 / 60.0; // minutes
            if time_diff > 0.0 {
                rank_change as f64 / time_diff
            } else {
                0.0
            }
        } else {
            0.0
        };

        let price_change_pct = ((position.current_price - position.entry_price) 
            / position.entry_price) * 100.0;
        let volume_change_pct = ((position.current_volume - position.entry_volume) 
            / position.entry_volume) * 100.0;

        let event = if previous_position.is_none() {
            // New entry
            Some(TrendingEvent::Entry { rank: position.rank })
        } else if let Some(prev_rank) = position.previous_rank {
            // Check for significant rank changes
            let rank_diff = (prev_rank as i32 - position.rank as i32).abs();
            if rank_diff >= 5 {
                if position.rank < prev_rank {
                    Some(TrendingEvent::RankUp {
                        old_rank: prev_rank,
                        new_rank: position.rank,
                    })
                } else {
                    Some(TrendingEvent::RankDown {
                        old_rank: prev_rank,
                        new_rank: position.rank,
                    })
                }
            } else if velocity.abs() > 2.0 {
                // High velocity
                Some(TrendingEvent::HighMomentum { velocity })
            } else {
                None
            }
        } else {
            None
        };

        if let Some(event) = event {
            let event_record = TrendingEventRecord {
                token_address: token_address.to_string(),
                event: event.clone(),
                position: position.clone(),
                duration_secs: duration,
                velocity,
                price_change_pct,
                volume_change_pct,
                timestamp: now,
            };

            info!(
                "Trending EVENT: {} - {:?} (rank={}, velocity={:.2}, price_change={:.1}%)",
                token_address, event, position.rank, velocity, price_change_pct
            );

            history
                .entry(token_address.to_string())
                .or_insert_with(|| TrendingHistory::new(self.config.max_history_size))
                .add_event(event_record);
        }

        // Always add position to history
        history
            .entry(token_address.to_string())
            .or_insert_with(|| TrendingHistory::new(self.config.max_history_size))
            .add_position(position.clone());
    }

    /// Calculate velocity (rank change per minute)
    async fn calculate_velocity(
        &self,
        token_address: &str,
        history: &tokio::sync::RwLockReadGuard<'_, HashMap<String, TrendingHistory>>,
    ) -> f64 {
        if let Some(hist) = history.get(token_address) {
            if hist.positions.len() < 2 {
                return 0.0;
            }

            let window_secs = self.config.velocity_window_secs;
            let now = hist.positions.back().unwrap().timestamp;
            let cutoff = now.saturating_sub(window_secs);

            let recent: Vec<_> = hist.positions.iter()
                .filter(|p| p.timestamp >= cutoff)
                .collect();

            if recent.len() < 2 {
                return 0.0;
            }

            let first = recent.first().unwrap();
            let last = recent.last().unwrap();
            let rank_change = first.rank as i32 - last.rank as i32;
            let time_diff = (last.timestamp - first.timestamp) as f64 / 60.0;

            if time_diff > 0.0 {
                rank_change as f64 / time_diff
            } else {
                0.0
            }
        } else {
            0.0
        }
    }

    /// Calculate momentum score (0-100)
    fn calculate_momentum_score(&self, position: &TrendingPosition, velocity: f64) -> f64 {
        let rank_score = (100.0 - position.rank as f64).max(0.0);
        let velocity_score = (velocity * 10.0).clamp(0.0, 50.0);
        let duration_score = (position.timestamp.saturating_sub(position.entry_timestamp) as f64 / 60.0)
            .min(30.0) * 1.67; // Max 50 points for 30 min

        ((rank_score + velocity_score + duration_score) / 2.0).clamp(0.0, 100.0)
    }

    /// Calculate price correlation (0-1)
    async fn calculate_price_correlation(
        &self,
        token_address: &str,
        history: &tokio::sync::RwLockReadGuard<'_, HashMap<String, TrendingHistory>>,
    ) -> f64 {
        if let Some(hist) = history.get(token_address) {
            if hist.positions.is_empty() {
                return 0.0;
            }

            let first = hist.positions.front().unwrap();
            let last = hist.positions.back().unwrap();

            if first.entry_price <= 0.0 {
                return 0.0;
            }

            let price_change = (last.current_price - first.entry_price) / first.entry_price;
            let rank_improvement = (first.rank as f64 - last.rank as f64) / first.rank as f64;

            // Positive correlation if both increase or both decrease
            if price_change * rank_improvement > 0.0 {
                (price_change.abs() + rank_improvement.abs()).min(1.0)
            } else {
                0.0
            }
        } else {
            0.0
        }
    }

    /// Calculate volume correlation (0-1)
    async fn calculate_volume_correlation(
        &self,
        token_address: &str,
        history: &tokio::sync::RwLockReadGuard<'_, HashMap<String, TrendingHistory>>,
    ) -> f64 {
        if let Some(hist) = history.get(token_address) {
            if hist.positions.is_empty() {
                return 0.0;
            }

            let first = hist.positions.front().unwrap();
            let last = hist.positions.back().unwrap();

            if first.entry_volume <= 0.0 {
                return 0.0;
            }

            let volume_change = (last.current_volume - first.entry_volume) / first.entry_volume;
            let rank_improvement = (first.rank as f64 - last.rank as f64) / first.rank as f64;

            // Positive correlation if both increase or both decrease
            if volume_change * rank_improvement > 0.0 {
                (volume_change.abs().min(1.0) + rank_improvement.abs()).min(1.0)
            } else {
                0.0
            }
        } else {
            0.0
        }
    }

    /// Get current trending rank for a token
    pub async fn get_trending_rank(&self, token_address: &str) -> Option<u32> {
        let positions = self.trending_positions.read().await;
        positions.get(token_address).map(|p| p.rank)
    }

    /// Check if token is currently trending
    pub async fn is_trending(&self, token_address: &str) -> bool {
        let positions = self.trending_positions.read().await;
        positions.contains_key(token_address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = DexScreenerConfig::default();
        assert_eq!(config.polling_interval_secs, 3);
        assert!(config.enable_tracking);
        assert_eq!(config.min_trending_rank, 100);
    }

    #[test]
    fn test_trending_metrics_default() {
        let metrics = TrendingMetrics::default();
        assert!(!metrics.is_trending);
        assert_eq!(metrics.current_rank, None);
        assert_eq!(metrics.duration_in_trending, 0);
    }

    #[tokio::test]
    async fn test_tracker_creation() {
        let config = DexScreenerConfig::default();
        let tracker = DexScreenerTracker::new(config);
        
        let metrics = tracker.get_trending_metrics("test_token").await.unwrap();
        assert!(!metrics.is_trending);
    }

    #[test]
    fn test_trending_history() {
        let mut history = TrendingHistory::new(100);
        
        let position = TrendingPosition {
            pair_address: "pair1".to_string(),
            token_address: "token1".to_string(),
            rank: 10,
            previous_rank: None,
            entry_timestamp: 1000,
            timestamp: 1000,
            entry_price: 1.0,
            current_price: 1.0,
            entry_volume: 10000.0,
            current_volume: 10000.0,
            liquidity_usd: 50000.0,
            market_cap_usd: Some(100000.0),
        };

        history.add_position(position);
        assert_eq!(history.positions.len(), 1);
    }
}
