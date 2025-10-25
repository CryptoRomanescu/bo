//! Cache layer for sentiment analysis results
//!
//! Provides high-performance caching for sentiment data with TTL support.

use super::types::{SentimentMetrics, TokenSentiment};
use moka::future::Cache;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info};

/// Sentiment cache with TTL support
#[derive(Clone)]
pub struct SentimentCache {
    /// Moka cache for token sentiment data
    cache: Cache<String, Arc<TokenSentiment>>,
    /// Metrics tracking
    metrics: Arc<Mutex<CacheMetrics>>,
    /// Cache TTL configuration
    ttl_seconds: u64,
}

#[derive(Debug, Default)]
struct CacheMetrics {
    hits: u64,
    misses: u64,
    updates: u64,
}

impl SentimentCache {
    /// Create a new sentiment cache
    pub fn new(max_capacity: usize, ttl_seconds: u64) -> Self {
        let cache = Cache::builder()
            .max_capacity(max_capacity as u64)
            .time_to_live(Duration::from_secs(ttl_seconds))
            .build();

        Self {
            cache,
            metrics: Arc::new(Mutex::new(CacheMetrics::default())),
            ttl_seconds,
        }
    }

    /// Get sentiment data for a token
    pub async fn get(&self, mint: &str) -> Option<Arc<TokenSentiment>> {
        let result = self.cache.get(mint).await;

        let mut metrics = self.metrics.lock().await;
        if result.is_some() {
            metrics.hits += 1;
            debug!("Cache hit for token: {}", mint);
        } else {
            metrics.misses += 1;
            debug!("Cache miss for token: {}", mint);
        }

        result
    }

    /// Store or update sentiment data for a token
    pub async fn set(&self, mint: String, sentiment: TokenSentiment) {
        let sentiment_arc = Arc::new(sentiment);
        self.cache.insert(mint.clone(), sentiment_arc).await;

        let mut metrics = self.metrics.lock().await;
        metrics.updates += 1;

        debug!("Updated cache for token: {}", mint);
    }

    /// Remove sentiment data for a token
    pub async fn remove(&self, mint: &str) {
        self.cache.invalidate(mint).await;
        debug!("Removed cache entry for token: {}", mint);
    }

    /// Clear all cached sentiment data
    pub async fn clear(&self) {
        self.cache.invalidate_all();
        info!("Cleared all sentiment cache entries");
    }

    /// Get cache statistics
    pub async fn get_metrics(&self) -> CacheMetricsSnapshot {
        let metrics = self.metrics.lock().await;
        let total_requests = metrics.hits + metrics.misses;
        let hit_rate = if total_requests > 0 {
            metrics.hits as f64 / total_requests as f64
        } else {
            0.0
        };

        CacheMetricsSnapshot {
            hits: metrics.hits,
            misses: metrics.misses,
            updates: metrics.updates,
            hit_rate,
            entry_count: self.cache.entry_count(),
            weighted_size: self.cache.weighted_size(),
        }
    }

    /// Get the configured TTL
    pub fn ttl_seconds(&self) -> u64 {
        self.ttl_seconds
    }
}

/// Snapshot of cache metrics
#[derive(Debug, Clone)]
pub struct CacheMetricsSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub updates: u64,
    pub hit_rate: f64,
    pub entry_count: u64,
    pub weighted_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::intelligence::types::{SentimentScore, SocialSource, TokenSentiment};

    #[tokio::test]
    async fn test_cache_basic_operations() {
        let cache = SentimentCache::new(100, 300);
        let mint = "test_mint".to_string();

        // Initially should be empty
        assert!(cache.get(&mint).await.is_none());

        // Add sentiment
        let mut sentiment = TokenSentiment::new(mint.clone());
        sentiment.total_mentions = 10;
        cache.set(mint.clone(), sentiment.clone()).await;

        // Should be retrievable
        let retrieved = cache.get(&mint).await.unwrap();
        assert_eq!(retrieved.total_mentions, 10);

        // Remove and verify
        cache.remove(&mint).await;
        assert!(cache.get(&mint).await.is_none());
    }

    #[tokio::test]
    async fn test_cache_metrics() {
        let cache = SentimentCache::new(100, 300);
        let mint = "test_mint".to_string();

        // Cause some cache misses
        cache.get(&mint).await;
        cache.get(&mint).await;

        // Add and retrieve (hit)
        let sentiment = TokenSentiment::new(mint.clone());
        cache.set(mint.clone(), sentiment).await;
        cache.get(&mint).await;

        let metrics = cache.get_metrics().await;
        assert_eq!(metrics.misses, 2);
        assert_eq!(metrics.hits, 1);
        assert_eq!(metrics.updates, 1);
        assert!(metrics.hit_rate > 0.0 && metrics.hit_rate < 1.0);
    }

    #[tokio::test]
    async fn test_cache_clear() {
        let cache = SentimentCache::new(100, 300);

        // Add multiple entries
        for i in 0..5 {
            let mint = format!("mint_{}", i);
            let sentiment = TokenSentiment::new(mint.clone());
            cache.set(mint, sentiment).await;
        }

        let metrics_before = cache.get_metrics().await;
        assert_eq!(metrics_before.entry_count, 5);

        // Clear cache
        cache.clear().await;

        let metrics_after = cache.get_metrics().await;
        assert_eq!(metrics_after.entry_count, 0);
    }

    #[tokio::test]
    async fn test_cache_update() {
        let cache = SentimentCache::new(100, 300);
        let mint = "test_mint".to_string();

        // Add initial sentiment
        let mut sentiment = TokenSentiment::new(mint.clone());
        sentiment.total_mentions = 5;
        cache.set(mint.clone(), sentiment).await;

        // Update with new data
        let mut updated_sentiment = TokenSentiment::new(mint.clone());
        updated_sentiment.total_mentions = 10;
        cache.set(mint.clone(), updated_sentiment).await;

        // Verify update
        let retrieved = cache.get(&mint).await.unwrap();
        assert_eq!(retrieved.total_mentions, 10);
    }
}
