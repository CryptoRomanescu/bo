//! Main sentiment analysis engine
//!
//! Orchestrates Twitter, Telegram, and Discord monitoring with sentiment scoring.

use super::discord_monitor::DiscordMonitor;
use super::sentiment_cache::SentimentCache;
use super::telegram_scraper::TelegramScraper;
use super::twitter_client::TwitterClient;
use super::types::{
    SentimentConfig, SentimentData, SentimentMetrics, SentimentScore, SocialSource, TokenSentiment,
};
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{debug, info, instrument, warn};

/// Main sentiment analysis engine
pub struct SentimentEngine {
    /// Twitter client
    twitter_client: Option<Arc<TwitterClient>>,
    /// Telegram scraper
    telegram_scraper: Option<Arc<TelegramScraper>>,
    /// Discord monitor
    discord_monitor: Option<Arc<DiscordMonitor>>,
    /// Sentiment cache
    cache: Arc<SentimentCache>,
    /// Configuration
    config: SentimentConfig,
    /// Performance metrics
    metrics: Arc<Mutex<SentimentMetrics>>,
}

impl SentimentEngine {
    /// Create a new sentiment engine
    pub fn new(config: SentimentConfig) -> Result<Self> {
        // Create cache
        let cache = Arc::new(SentimentCache::new(
            config.max_tracked_tokens,
            config.cache_ttl_seconds,
        ));

        // Create Twitter client if enabled
        let twitter_client = if config.enable_twitter {
            match TwitterClient::new(config.clone()) {
                Ok(client) => {
                    info!("Twitter client initialized successfully");
                    Some(Arc::new(client))
                }
                Err(e) => {
                    warn!("Failed to initialize Twitter client: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Create Telegram scraper if enabled
        let telegram_scraper = if config.enable_telegram {
            match TelegramScraper::new(config.clone()) {
                Ok(scraper) => {
                    info!("Telegram scraper initialized successfully");
                    Some(Arc::new(scraper))
                }
                Err(e) => {
                    warn!("Failed to initialize Telegram scraper: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Create Discord monitor if enabled
        let discord_monitor = if config.enable_discord {
            match DiscordMonitor::new(config.clone()) {
                Ok(monitor) => {
                    info!("Discord monitor initialized successfully");
                    Some(Arc::new(monitor))
                }
                Err(e) => {
                    warn!("Failed to initialize Discord monitor: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            twitter_client,
            telegram_scraper,
            discord_monitor,
            cache,
            config,
            metrics: Arc::new(Mutex::new(SentimentMetrics::default())),
        })
    }

    /// Get sentiment for a token (checks cache first)
    #[instrument(skip(self))]
    pub async fn get_token_sentiment(&self, mint: &str) -> Result<TokenSentiment> {
        let start_time = Instant::now();

        // Check cache first
        if let Some(cached) = self.cache.get(mint).await {
            let mut metrics = self.metrics.lock().await;
            metrics.total_analyses += 1;
            metrics.avg_processing_time_ms = (metrics.avg_processing_time_ms
                * (metrics.total_analyses - 1) as f64
                + start_time.elapsed().as_millis() as f64)
                / metrics.total_analyses as f64;

            debug!("Retrieved sentiment from cache for token: {}", mint);
            return Ok((*cached).clone());
        }

        // Not in cache, fetch fresh data
        let sentiment = self.fetch_fresh_sentiment(mint).await?;

        // Update cache
        self.cache.set(mint.to_string(), sentiment.clone()).await;

        // Update metrics
        let mut metrics = self.metrics.lock().await;
        metrics.total_analyses += 1;
        metrics.avg_processing_time_ms = (metrics.avg_processing_time_ms
            * (metrics.total_analyses - 1) as f64
            + start_time.elapsed().as_millis() as f64)
            / metrics.total_analyses as f64;
        metrics.tracked_tokens_count = self.cache.get_metrics().await.entry_count as usize;

        Ok(sentiment)
    }

    /// Fetch fresh sentiment data from all sources
    #[instrument(skip(self))]
    async fn fetch_fresh_sentiment(&self, mint: &str) -> Result<TokenSentiment> {
        let mut token_sentiment = TokenSentiment::new(mint.to_string());
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        let mut all_data = Vec::new();

        // Fetch from Twitter
        if let Some(ref twitter) = self.twitter_client {
            if twitter.is_enabled() {
                match twitter.search_tweets(&format!("${}", mint), 10).await {
                    Ok(mut data) => {
                        info!("Fetched {} tweets for token: {}", data.len(), mint);
                        all_data.append(&mut data);
                    }
                    Err(e) => {
                        warn!("Failed to fetch Twitter data for {}: {}", mint, e);
                        let mut metrics = self.metrics.lock().await;
                        metrics.error_count += 1;
                    }
                }
            }
        }

        // Fetch from Telegram
        if let Some(ref telegram) = self.telegram_scraper {
            if telegram.is_enabled() {
                match telegram.search_messages(vec![mint.to_string()]).await {
                    Ok(mut data) => {
                        info!(
                            "Fetched {} Telegram messages for token: {}",
                            data.len(),
                            mint
                        );
                        all_data.append(&mut data);
                    }
                    Err(e) => {
                        warn!("Failed to fetch Telegram data for {}: {}", mint, e);
                        let mut metrics = self.metrics.lock().await;
                        metrics.error_count += 1;
                    }
                }
            }
        }

        // Fetch from Discord
        if let Some(ref discord) = self.discord_monitor {
            if discord.is_enabled() {
                match discord
                    .monitor_tracked_channels(vec![mint.to_string()])
                    .await
                {
                    Ok(mut data) => {
                        info!(
                            "Fetched {} Discord messages for token: {}",
                            data.len(),
                            mint
                        );
                        all_data.append(&mut data);
                    }
                    Err(e) => {
                        warn!("Failed to fetch Discord data for {}: {}", mint, e);
                        let mut metrics = self.metrics.lock().await;
                        metrics.error_count += 1;
                    }
                }
            }
        }

        // Analyze sentiment for all collected data
        for data in all_data {
            let score = self.analyze_sentiment(&data);
            token_sentiment.add_score(score, timestamp);
        }

        info!(
            "Sentiment analysis complete for {}: score={:.3}, mentions={}",
            mint, token_sentiment.overall_score, token_sentiment.total_mentions
        );

        Ok(token_sentiment)
    }

    /// Analyze sentiment of a single piece of content
    #[instrument(skip(self, data))]
    fn analyze_sentiment(&self, data: &SentimentData) -> SentimentScore {
        // Simple keyword-based sentiment analysis
        // In production, you'd use a proper NLP model
        let content_lower = data.content.to_lowercase();

        // Positive keywords
        let positive_words = vec![
            "moon", "bullish", "buy", "gem", "amazing", "great", "love", "pump", "rocket", "hold",
            "strong", "good", "best", "profit", "win", "success", "up", "high", "rich", "ath",
            "breakout",
        ];

        // Negative keywords
        let negative_words = vec![
            "dump", "scam", "rug", "bear", "sell", "crash", "bad", "loss", "down", "low", "fail",
            "dead", "terrible", "avoid", "warning", "red", "bearish", "broke", "rekt",
        ];

        let mut positive_count = 0;
        let mut negative_count = 0;

        for word in positive_words {
            if content_lower.contains(word) {
                positive_count += 1;
            }
        }

        for word in negative_words {
            if content_lower.contains(word) {
                negative_count += 1;
            }
        }

        let total_count = positive_count + negative_count;

        // Calculate raw score
        let raw_score = if total_count > 0 {
            (positive_count as f64 - negative_count as f64) / total_count as f64
        } else {
            0.0 // Neutral if no sentiment keywords found
        };

        // Adjust confidence based on engagement
        let base_confidence = if total_count > 0 { 0.6 } else { 0.3 };
        let engagement_boost = (data.engagement_count as f64).ln().max(0.0) / 10.0;
        let confidence = (base_confidence + engagement_boost).min(1.0);

        SentimentScore {
            score: raw_score,
            confidence,
            timestamp: data.timestamp,
            source: data.source,
        }
    }

    /// Get sentiment score normalized to 0.0-1.0 range for Oracle integration
    #[instrument(skip(self))]
    pub async fn get_normalized_sentiment_score(&self, mint: &str) -> Result<f64> {
        let sentiment = self.get_token_sentiment(mint).await?;

        // Check if we have enough mentions
        if sentiment.total_mentions < self.config.min_mentions_threshold {
            debug!(
                "Insufficient mentions ({}) for token: {}",
                sentiment.total_mentions, mint
            );
            return Ok(0.5); // Neutral score if not enough data
        }

        // Convert from -1.0...1.0 to 0.0...1.0
        let normalized = (sentiment.overall_score + 1.0) / 2.0;

        // Weight by confidence
        let weighted_score = normalized * sentiment.overall_confidence;

        Ok(weighted_score.clamp(0.0, 1.0))
    }

    /// Get metrics
    pub async fn get_metrics(&self) -> SentimentMetrics {
        let mut metrics = self.metrics.lock().await.clone();

        // Update cache metrics
        let cache_metrics = self.cache.get_metrics().await;
        metrics.cache_hit_rate = cache_metrics.hit_rate;
        metrics.tracked_tokens_count = cache_metrics.entry_count as usize;

        // Update API call counts
        let mut total_api_calls = 0;
        if let Some(ref twitter) = self.twitter_client {
            total_api_calls += twitter.get_api_call_count().await;
        }
        if let Some(ref telegram) = self.telegram_scraper {
            total_api_calls += telegram.get_api_call_count().await;
        }
        if let Some(ref discord) = self.discord_monitor {
            total_api_calls += discord.get_api_call_count().await;
        }
        metrics.total_api_calls = total_api_calls;

        metrics
    }

    /// Track a Discord channel
    pub async fn track_discord_channel(&self, channel_id: String) -> Result<()> {
        if let Some(ref monitor) = self.discord_monitor {
            monitor.track_channel(channel_id).await;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Discord monitor not initialized"))
        }
    }

    /// Clear sentiment cache
    pub async fn clear_cache(&self) {
        self.cache.clear().await;
        info!("Sentiment cache cleared");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> SentimentConfig {
        SentimentConfig {
            enable_twitter: false, // Disable for tests
            enable_telegram: false,
            enable_discord: false,
            max_tracked_tokens: 100,
            cache_ttl_seconds: 300,
            min_mentions_threshold: 5,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_sentiment_engine_creation() {
        let config = create_test_config();
        let engine = SentimentEngine::new(config);
        assert!(engine.is_ok());
    }

    #[tokio::test]
    async fn test_analyze_sentiment_positive() {
        let config = create_test_config();
        let engine = SentimentEngine::new(config).unwrap();

        let data = SentimentData {
            token_identifier: "TEST".to_string(),
            content: "This token is amazing! Going to the moon! Bullish!".to_string(),
            source: SocialSource::Twitter,
            timestamp: 1000000,
            author_id: "test_user".to_string(),
            engagement_count: 50,
        };

        let score = engine.analyze_sentiment(&data);
        assert!(score.score > 0.0, "Should be positive sentiment");
        assert!(score.confidence > 0.0);
    }

    #[tokio::test]
    async fn test_analyze_sentiment_negative() {
        let config = create_test_config();
        let engine = SentimentEngine::new(config).unwrap();

        let data = SentimentData {
            token_identifier: "TEST".to_string(),
            content: "This is a scam! Rug pull incoming! Avoid this terrible project!".to_string(),
            source: SocialSource::Twitter,
            timestamp: 1000000,
            author_id: "test_user".to_string(),
            engagement_count: 20,
        };

        let score = engine.analyze_sentiment(&data);
        assert!(score.score < 0.0, "Should be negative sentiment");
    }

    #[tokio::test]
    async fn test_analyze_sentiment_neutral() {
        let config = create_test_config();
        let engine = SentimentEngine::new(config).unwrap();

        let data = SentimentData {
            token_identifier: "TEST".to_string(),
            content: "Here is some information about the token".to_string(),
            source: SocialSource::Twitter,
            timestamp: 1000000,
            author_id: "test_user".to_string(),
            engagement_count: 5,
        };

        let score = engine.analyze_sentiment(&data);
        assert_eq!(score.score, 0.0, "Should be neutral sentiment");
    }

    #[tokio::test]
    async fn test_get_metrics() {
        let config = create_test_config();
        let engine = SentimentEngine::new(config).unwrap();

        let metrics = engine.get_metrics().await;
        assert_eq!(metrics.total_analyses, 0);
        assert_eq!(metrics.error_count, 0);
    }
}
