//! TTL Cache for Early Pump Analysis Results
//!
//! Provides high-performance caching for early pump analysis results with:
//! - TTL (Time-To-Live) support (default: 60s)
//! - Automatic invalidation on new transactions/token accounts
//! - Force refresh capability
//! - Cache hit/miss metrics
//! - Thread-safe async operations
//!
//! ## Features
//! - Moka-based in-memory cache with automatic TTL expiration
//! - Real-time metrics tracking (hits, misses, updates)
//! - Support for cache invalidation triggers
//! - Configurable capacity and TTL

use crate::oracle::early_pump_detector::EarlyPumpAnalysis;
use moka::future::Cache;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info};

/// Cache for early pump analysis results
#[derive(Clone)]
pub struct EarlyPumpCache {
    /// Moka cache for analysis results
    cache: Cache<String, Arc<EarlyPumpAnalysis>>,
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
    invalidations: u64,
    force_refreshes: u64,
}

impl EarlyPumpCache {
    /// Create a new early pump cache
    ///
    /// # Arguments
    /// * `max_capacity` - Maximum number of entries in cache
    /// * `ttl_seconds` - Time-to-live for cache entries (default: 60s)
    pub fn new(max_capacity: usize, ttl_seconds: u64) -> Self {
        let cache = Cache::builder()
            .max_capacity(max_capacity as u64)
            .time_to_live(Duration::from_secs(ttl_seconds))
            .build();

        info!(
            "Initialized EarlyPumpCache with max_capacity={}, ttl={}s",
            max_capacity, ttl_seconds
        );

        Self {
            cache,
            metrics: Arc::new(Mutex::new(CacheMetrics::default())),
            ttl_seconds,
        }
    }

    /// Get cached analysis for a token
    ///
    /// # Arguments
    /// * `mint` - Token mint address
    ///
    /// # Returns
    /// Cached analysis if available and not expired, None otherwise
    pub async fn get(&self, mint: &str) -> Option<Arc<EarlyPumpAnalysis>> {
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

    /// Store or update analysis for a token
    ///
    /// # Arguments
    /// * `mint` - Token mint address
    /// * `analysis` - Analysis result to cache
    pub async fn set(&self, mint: String, analysis: EarlyPumpAnalysis) {
        let analysis_arc = Arc::new(analysis);
        self.cache.insert(mint.clone(), analysis_arc).await;

        let mut metrics = self.metrics.lock().await;
        metrics.updates += 1;

        debug!("Updated cache for token: {}", mint);
    }

    /// Invalidate cache entry for a token
    ///
    /// Used when new transactions or token accounts are detected
    ///
    /// # Arguments
    /// * `mint` - Token mint address
    pub async fn invalidate(&self, mint: &str) {
        self.cache.invalidate(mint).await;

        let mut metrics = self.metrics.lock().await;
        metrics.invalidations += 1;

        debug!("Invalidated cache entry for token: {}", mint);
    }

    /// Force refresh - clear cache entry to force re-analysis
    ///
    /// # Arguments
    /// * `mint` - Token mint address
    pub async fn force_refresh(&self, mint: &str) {
        self.cache.invalidate(mint).await;

        let mut metrics = self.metrics.lock().await;
        metrics.force_refreshes += 1;

        info!("Force refresh for token: {}", mint);
    }

    /// Clear all cached entries
    pub async fn clear(&self) {
        self.cache.invalidate_all();
        info!("Cleared all cache entries");
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
            invalidations: metrics.invalidations,
            force_refreshes: metrics.force_refreshes,
            hit_rate,
            entry_count: self.cache.entry_count(),
            weighted_size: self.cache.weighted_size(),
        }
    }

    /// Get the configured TTL
    pub fn ttl_seconds(&self) -> u64 {
        self.ttl_seconds
    }

    /// Check if a token has a cached analysis
    pub async fn contains(&self, mint: &str) -> bool {
        self.cache.get(mint).await.is_some()
    }
}

/// Snapshot of cache metrics
#[derive(Debug, Clone)]
pub struct CacheMetricsSnapshot {
    /// Number of cache hits
    pub hits: u64,
    /// Number of cache misses
    pub misses: u64,
    /// Number of cache updates
    pub updates: u64,
    /// Number of invalidations (triggered by new data)
    pub invalidations: u64,
    /// Number of force refreshes (manual)
    pub force_refreshes: u64,
    /// Cache hit rate (0.0-1.0)
    pub hit_rate: f64,
    /// Current number of entries in cache
    pub entry_count: u64,
    /// Weighted size of cache
    pub weighted_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::early_pump_detector::{
        CheckResults, CheckTimings, DecisionTimings, PumpDecision,
    };

    fn create_test_analysis(mint: &str, score: u8) -> EarlyPumpAnalysis {
        let now = chrono::Utc::now().timestamp() as u64;
        EarlyPumpAnalysis {
            mint: mint.to_string(),
            deploy_timestamp: now - 60,
            detection_timestamp: now - 30,
            decision_timestamp: now,
            decision: PumpDecision::Buy {
                score,
                reason: "Test".to_string(),
            },
            score,
            timings: DecisionTimings {
                deploy_to_detection_ms: 30000,
                detection_to_decision_ms: 30000,
                total_decision_time_ms: 60000,
                check_timings: CheckTimings::default(),
            },
            check_results: CheckResults::default(),
        }
    }

    #[tokio::test]
    async fn test_cache_basic_operations() {
        let cache = EarlyPumpCache::new(100, 60);
        let mint = "test_mint";

        // Initially should be empty
        assert!(!cache.contains(mint).await);
        assert!(cache.get(mint).await.is_none());

        // Add analysis
        let analysis = create_test_analysis(mint, 75);
        cache.set(mint.to_string(), analysis.clone()).await;

        // Should be retrievable
        assert!(cache.contains(mint).await);
        let retrieved = cache.get(mint).await.unwrap();
        assert_eq!(retrieved.mint, mint);
        assert_eq!(retrieved.score, 75);

        // Invalidate and verify
        cache.invalidate(mint).await;
        assert!(!cache.contains(mint).await);
        assert!(cache.get(mint).await.is_none());
    }

    #[tokio::test]
    async fn test_cache_metrics() {
        let cache = EarlyPumpCache::new(100, 60);
        let mint = "test_mint";

        // Cause some cache misses
        cache.get(mint).await;
        cache.get(mint).await;

        // Add and retrieve (hit)
        let analysis = create_test_analysis(mint, 80);
        cache.set(mint.to_string(), analysis).await;
        cache.get(mint).await;

        let metrics = cache.get_metrics().await;
        assert_eq!(metrics.misses, 2);
        assert_eq!(metrics.hits, 1);
        assert_eq!(metrics.updates, 1);
        assert!(metrics.hit_rate > 0.0 && metrics.hit_rate < 1.0);
    }

    #[tokio::test]
    async fn test_cache_invalidation() {
        let cache = EarlyPumpCache::new(100, 60);
        let mint = "test_mint";

        // Add analysis
        let analysis = create_test_analysis(mint, 70);
        cache.set(mint.to_string(), analysis).await;

        // Verify it's cached
        assert!(cache.contains(mint).await);

        // Invalidate
        cache.invalidate(mint).await;

        // Verify it's removed
        assert!(!cache.contains(mint).await);

        // Check metrics
        let metrics = cache.get_metrics().await;
        assert_eq!(metrics.invalidations, 1);
    }

    #[tokio::test]
    async fn test_force_refresh() {
        let cache = EarlyPumpCache::new(100, 60);
        let mint = "test_mint";

        // Add analysis
        let analysis = create_test_analysis(mint, 85);
        cache.set(mint.to_string(), analysis).await;

        // Force refresh
        cache.force_refresh(mint).await;

        // Verify it's removed
        assert!(!cache.contains(mint).await);

        // Check metrics
        let metrics = cache.get_metrics().await;
        assert_eq!(metrics.force_refreshes, 1);
    }

    #[tokio::test]
    async fn test_cache_clear() {
        let cache = EarlyPumpCache::new(100, 60);

        // Add multiple entries
        let mut mints = Vec::new();
        for i in 0..5 {
            let mint = format!("mint_{}", i);
            let analysis = create_test_analysis(&mint, 60 + i as u8);
            cache.set(mint.clone(), analysis).await;
            mints.push(mint);
        }

        // Wait for cache to sync
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Verify all entries exist
        for mint in &mints {
            assert!(cache.contains(mint).await);
        }

        // Clear cache
        cache.clear().await;

        // Wait for invalidation to propagate
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Verify all entries are removed
        for mint in &mints {
            assert!(!cache.contains(mint).await);
        }
    }

    #[tokio::test]
    async fn test_cache_update() {
        let cache = EarlyPumpCache::new(100, 60);
        let mint = "test_mint";

        // Add initial analysis
        let analysis1 = create_test_analysis(mint, 60);
        cache.set(mint.to_string(), analysis1).await;

        // Update with new analysis
        let analysis2 = create_test_analysis(mint, 90);
        cache.set(mint.to_string(), analysis2).await;

        // Verify update
        let retrieved = cache.get(mint).await.unwrap();
        assert_eq!(retrieved.score, 90);

        // Check metrics - should have 2 updates
        let metrics = cache.get_metrics().await;
        assert_eq!(metrics.updates, 2);
    }

    #[tokio::test]
    async fn test_ttl_expiration() {
        // Create cache with 1 second TTL
        let cache = EarlyPumpCache::new(100, 1);
        let mint = "test_mint";

        // Add analysis
        let analysis = create_test_analysis(mint, 75);
        cache.set(mint.to_string(), analysis).await;

        // Should be available immediately
        assert!(cache.contains(mint).await);

        // Wait for TTL to expire
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Should be expired
        assert!(!cache.contains(mint).await);
    }
}
