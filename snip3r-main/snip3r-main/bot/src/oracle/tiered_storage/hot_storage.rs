//! Hot Memory Storage (L1 Tier)
//!
//! In-memory LRU cache with automatic eviction for frequently accessed data.
//! Target access time: <1ms

use anyhow::{Context, Result};
use async_trait::async_trait;
use moka::future::Cache;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info};

use super::traits::{StorageTier, TierMetricsSnapshot};
use crate::oracle::types::TransactionRecord;

/// Hot storage tier using in-memory LRU cache
///
/// This tier provides sub-millisecond access to frequently used records.
/// It automatically evicts least recently used items when capacity is reached.
pub struct HotMemoryStorage {
    /// LRU cache for records by ID
    cache_by_id: Cache<i64, Arc<TransactionRecord>>,

    /// LRU cache for records by signature
    cache_by_signature: Cache<String, Arc<TransactionRecord>>,

    /// Access metrics
    metrics: Arc<RwLock<HotStorageMetrics>>,

    /// Maximum capacity (number of records)
    max_capacity: u64,
}

#[derive(Debug, Default)]
struct HotStorageMetrics {
    access_count: u64,
    hit_count: u64,
    miss_count: u64,
    eviction_count: u64,
    access_times_us: Vec<u64>, // Store recent access times for percentile calculation
}

impl HotMemoryStorage {
    /// Create a new hot storage tier with the specified capacity
    ///
    /// # Arguments
    /// * `max_capacity` - Maximum number of records to store in memory
    pub fn new(max_capacity: u64) -> Self {
        info!(
            "Initializing HotMemoryStorage with capacity: {}",
            max_capacity
        );

        Self {
            cache_by_id: Cache::builder()
                .max_capacity(max_capacity)
                .time_to_live(Duration::from_secs(3600)) // 1 hour TTL
                .build(),
            cache_by_signature: Cache::builder()
                .max_capacity(max_capacity)
                .time_to_live(Duration::from_secs(3600))
                .build(),
            metrics: Arc::new(RwLock::new(HotStorageMetrics::default())),
            max_capacity,
        }
    }

    /// Create with default capacity (10,000 records)
    pub fn with_default_capacity() -> Self {
        Self::new(10_000)
    }

    /// Run pending cache maintenance tasks
    /// This should be called after batch inserts for testing
    pub async fn sync_cache(&self) {
        self.cache_by_id.run_pending_tasks().await;
        self.cache_by_signature.run_pending_tasks().await;
    }

    /// Record an access time for metrics
    async fn record_access(&self, duration_us: u64, hit: bool) {
        let mut metrics = self.metrics.write().await;
        metrics.access_count += 1;
        if hit {
            metrics.hit_count += 1;
        } else {
            metrics.miss_count += 1;
        }

        // Keep only the last 1000 access times for percentile calculation
        metrics.access_times_us.push(duration_us);
        if metrics.access_times_us.len() > 1000 {
            metrics.access_times_us.remove(0);
        }
    }

    /// Calculate percentile from sorted access times
    fn calculate_percentile(sorted_times: &[u64], percentile: f64) -> u64 {
        if sorted_times.is_empty() {
            return 0;
        }
        let index = ((sorted_times.len() as f64 - 1.0) * percentile / 100.0) as usize;
        sorted_times[index]
    }
}

#[async_trait]
impl StorageTier for HotMemoryStorage {
    async fn get(&self, id: i64) -> Result<Option<TransactionRecord>> {
        let start = Instant::now();

        let result = self.cache_by_id.get(&id).await;

        let duration_us = start.elapsed().as_micros() as u64;
        self.record_access(duration_us, result.is_some()).await;

        if result.is_some() {
            debug!("Hot storage hit for ID: {} in {}µs", id, duration_us);
        }

        Ok(result.as_ref().map(|arc| (**arc).clone()))
    }

    async fn get_by_signature(&self, signature: &str) -> Result<Option<TransactionRecord>> {
        let start = Instant::now();

        let result = self.cache_by_signature.get(signature).await;

        let duration_us = start.elapsed().as_micros() as u64;
        self.record_access(duration_us, result.is_some()).await;

        if result.is_some() {
            debug!(
                "Hot storage hit for signature: {} in {}µs",
                signature, duration_us
            );
        }

        Ok(result.as_ref().map(|arc| (**arc).clone()))
    }

    async fn put(&self, record: &TransactionRecord) -> Result<i64> {
        let record_arc = Arc::new(record.clone());

        // Store by ID if available
        if let Some(id) = record.id {
            self.cache_by_id.insert(id, record_arc.clone()).await;
        }

        // Store by signature if available
        if let Some(ref signature) = record.transaction_signature {
            self.cache_by_signature
                .insert(signature.clone(), record_arc)
                .await;
        }

        Ok(record.id.unwrap_or(0))
    }

    async fn remove(&self, id: i64) -> Result<bool> {
        // Get the record first to find its signature
        if let Some(record) = self.cache_by_id.get(&id).await {
            if let Some(ref signature) = record.transaction_signature {
                self.cache_by_signature.invalidate(signature).await;
            }
            self.cache_by_id.invalidate(&id).await;

            let mut metrics = self.metrics.write().await;
            metrics.eviction_count += 1;

            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn list_since(&self, timestamp: u64) -> Result<Vec<TransactionRecord>> {
        // For hot storage, we need to iterate through all cached items
        // This is not the primary use case for L1 cache
        let mut results = Vec::new();

        // Iterate through cache (note: moka doesn't provide direct iteration,
        // so this is a simplified implementation)
        // In practice, this operation should be rare for hot storage

        Ok(results)
    }

    async fn count(&self) -> Result<u64> {
        Ok(self.cache_by_id.entry_count())
    }

    async fn contains(&self, id: i64) -> Result<bool> {
        Ok(self.cache_by_id.contains_key(&id))
    }

    async fn get_metrics(&self) -> Result<TierMetricsSnapshot> {
        let metrics = self.metrics.read().await;

        // Calculate percentiles
        let mut sorted_times = metrics.access_times_us.clone();
        sorted_times.sort_unstable();

        let avg_access_time_us = if !sorted_times.is_empty() {
            sorted_times.iter().sum::<u64>() / sorted_times.len() as u64
        } else {
            0
        };

        let p95 = Self::calculate_percentile(&sorted_times, 95.0);
        let p99 = Self::calculate_percentile(&sorted_times, 99.0);

        // Estimate bytes stored (rough estimate: 2KB per record)
        let record_count = self.cache_by_id.entry_count();
        let bytes_stored = record_count * 2048;

        Ok(TierMetricsSnapshot {
            record_count,
            access_count: metrics.access_count,
            hit_count: metrics.hit_count,
            miss_count: metrics.miss_count,
            avg_access_time_us,
            p95_access_time_us: p95,
            p99_access_time_us: p99,
            bytes_stored,
            eviction_count: metrics.eviction_count,
        })
    }

    async fn maintenance(&self) -> Result<()> {
        // Run cache synchronization
        self.cache_by_id.run_pending_tasks().await;
        self.cache_by_signature.run_pending_tasks().await;

        debug!("Hot storage maintenance completed");
        Ok(())
    }

    fn tier_name(&self) -> &'static str {
        "Hot (L1)"
    }

    fn target_access_time(&self) -> Duration {
        Duration::from_micros(1000) // 1ms target
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::types::{Outcome, ScoredCandidate};
    use crate::types::PremintCandidate;
    use std::collections::HashMap;

    fn create_test_record(id: i64, signature: &str) -> TransactionRecord {
        let scored_candidate = ScoredCandidate {
            base: PremintCandidate {
                mint: format!("test_mint_{}", id),
                creator: "creator".to_string(),
                program: "program".to_string(),
                slot: 0,
                timestamp: 1000,
                instruction_summary: None,
                is_jito_bundle: None,
            },
            mint: format!("test_mint_{}", id),
            predicted_score: 80,
            reason: "test".to_string(),
            feature_scores: HashMap::new(),
            calculation_time: 100,
            anomaly_detected: false,
            timestamp: 1000,
        };

        TransactionRecord {
            id: Some(id),
            scored_candidate,
            transaction_signature: Some(signature.to_string()),
            buy_price_sol: Some(0.001),
            sell_price_sol: None,
            amount_bought_tokens: Some(1000.0),
            amount_sold_tokens: None,
            initial_sol_spent: Some(1.0),
            final_sol_received: None,
            timestamp_decision_made: 1000,
            timestamp_transaction_sent: None,
            timestamp_outcome_evaluated: None,
            actual_outcome: Outcome::PendingConfirmation,
            market_context_snapshot: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_hot_storage_put_and_get() {
        let storage = HotMemoryStorage::new(100);
        let record = create_test_record(1, "sig123");

        // Put record
        storage.put(&record).await.unwrap();

        // Get by ID
        let retrieved = storage.get(1).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, Some(1));

        // Get by signature
        let retrieved = storage.get_by_signature("sig123").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(
            retrieved.unwrap().transaction_signature,
            Some("sig123".to_string())
        );
    }

    #[tokio::test]
    async fn test_hot_storage_access_time_under_1ms() {
        let storage = HotMemoryStorage::new(100);
        let record = create_test_record(1, "sig_fast");

        storage.put(&record).await.unwrap();

        let start = Instant::now();
        let _ = storage.get(1).await.unwrap();
        let duration = start.elapsed();

        // Should be well under 1ms for in-memory access
        assert!(
            duration < Duration::from_millis(1),
            "Access time {}µs exceeded 1ms target",
            duration.as_micros()
        );
    }

    #[tokio::test]
    async fn test_hot_storage_metrics() {
        let storage = HotMemoryStorage::new(100);
        let record = create_test_record(1, "sig_metrics");

        storage.put(&record).await.unwrap();

        // Hit
        let _ = storage.get(1).await.unwrap();

        // Miss
        let _ = storage.get(999).await.unwrap();

        let metrics = storage.get_metrics().await.unwrap();

        assert_eq!(metrics.access_count, 2);
        assert_eq!(metrics.hit_count, 1);
        assert_eq!(metrics.miss_count, 1);
        assert_eq!(metrics.hit_rate(), 50.0);
    }

    #[tokio::test]
    async fn test_hot_storage_remove() {
        let storage = HotMemoryStorage::new(100);
        let record = create_test_record(1, "sig_remove");

        storage.put(&record).await.unwrap();

        // Verify it exists
        assert!(storage.contains(1).await.unwrap());

        // Remove it
        let removed = storage.remove(1).await.unwrap();
        assert!(removed);

        // Verify it's gone
        assert!(!storage.contains(1).await.unwrap());
        let result = storage.get(1).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_hot_storage_count() {
        let storage = HotMemoryStorage::new(100);

        assert_eq!(storage.count().await.unwrap(), 0);

        for i in 1..=10 {
            let record = create_test_record(i, &format!("sig_{}", i));
            storage.put(&record).await.unwrap();
        }

        // Wait for cache to process inserts
        storage.sync_cache().await;

        let count = storage.count().await.unwrap();
        assert!(count >= 10, "Expected at least 10 records, got {}", count);
    }
}
