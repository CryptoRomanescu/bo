//! Auto-Tiering Coordinator
//!
//! Automatically moves data between storage tiers based on access patterns,
//! age, and tier capacity.

use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tracing::{debug, info, warn};

use super::cold_storage::ColdArchiveStorage;
use super::hot_storage::HotMemoryStorage;
use super::metrics::{TierMetrics, TierMonitor};
use super::traits::StorageTier;
use super::warm_storage::WarmSqliteStorage;
use crate::oracle::types::TransactionRecord;

/// Coordinator for automatic data movement between tiers
pub struct AutoTieringCoordinator {
    hot: Arc<HotMemoryStorage>,
    warm: Arc<WarmSqliteStorage>,
    cold: Arc<ColdArchiveStorage>,
    monitor: Arc<TierMonitor>,
    config: TieringConfig,
}

/// Configuration for auto-tiering behavior
#[derive(Debug, Clone)]
pub struct TieringConfig {
    /// Age threshold for moving from hot to warm (in seconds)
    pub hot_to_warm_age: u64,

    /// Age threshold for moving from warm to cold (in seconds)
    pub warm_to_cold_age: u64,

    /// Interval for running tiering maintenance (in seconds)
    pub maintenance_interval: u64,

    /// Enable automatic promotion to hot tier on access
    pub auto_promote_on_access: bool,

    /// Minimum access count for keeping in hot tier
    pub min_hot_access_count: u64,
}

impl Default for TieringConfig {
    fn default() -> Self {
        Self {
            hot_to_warm_age: 3600,       // 1 hour
            warm_to_cold_age: 86400 * 7, // 7 days
            maintenance_interval: 300,   // 5 minutes
            auto_promote_on_access: true,
            min_hot_access_count: 2,
        }
    }
}

impl AutoTieringCoordinator {
    /// Create a new auto-tiering coordinator
    pub async fn new(config: TieringConfig) -> Result<Self> {
        info!("Initializing AutoTieringCoordinator");

        let hot = Arc::new(HotMemoryStorage::with_default_capacity());
        let warm = Arc::new(WarmSqliteStorage::with_defaults().await?);
        let cold = Arc::new(ColdArchiveStorage::with_defaults().await?);
        let monitor = Arc::new(TierMonitor::new());

        Ok(Self {
            hot,
            warm,
            cold,
            monitor,
            config,
        })
    }

    /// Create with default configuration
    pub async fn with_defaults() -> Result<Self> {
        Self::new(TieringConfig::default()).await
    }

    /// Create for testing with unique storage paths
    #[cfg(test)]
    pub async fn with_test_defaults() -> Result<Self> {
        let hot = Arc::new(HotMemoryStorage::with_default_capacity());
        let warm = Arc::new(WarmSqliteStorage::with_test_defaults().await?);
        let cold_path = format!("/tmp/cold_test_{}", rand::random::<u64>());
        let cold = Arc::new(ColdArchiveStorage::new(&cold_path).await?);
        let monitor = Arc::new(TierMonitor::new());

        Ok(Self {
            hot,
            warm,
            cold,
            monitor,
            config: TieringConfig::default(),
        })
    }

    /// Get a record, checking all tiers from hot to cold
    pub async fn get(&self, id: i64) -> Result<Option<TransactionRecord>> {
        // Try hot tier first
        if let Some(record) = self.hot.get(id).await? {
            debug!("Record {} found in hot tier", id);
            return Ok(Some(record));
        }

        // Try warm tier
        if let Some(record) = self.warm.get(id).await? {
            debug!("Record {} found in warm tier", id);

            // Auto-promote to hot tier if configured
            if self.config.auto_promote_on_access {
                let _ = self.hot.put(&record).await;
                debug!("Auto-promoted record {} to hot tier", id);
            }

            return Ok(Some(record));
        }

        // Try cold tier
        if let Some(record) = self.cold.get(id).await? {
            debug!("Record {} found in cold tier", id);

            // Auto-promote to hot tier if configured
            if self.config.auto_promote_on_access {
                let _ = self.hot.put(&record).await;
                debug!("Auto-promoted record {} to hot tier", id);
            }

            return Ok(Some(record));
        }

        debug!("Record {} not found in any tier", id);
        Ok(None)
    }

    /// Get a record by signature
    pub async fn get_by_signature(&self, signature: &str) -> Result<Option<TransactionRecord>> {
        // Try hot tier first
        if let Some(record) = self.hot.get_by_signature(signature).await? {
            return Ok(Some(record));
        }

        // Try warm tier
        if let Some(record) = self.warm.get_by_signature(signature).await? {
            if self.config.auto_promote_on_access {
                let _ = self.hot.put(&record).await;
            }
            return Ok(Some(record));
        }

        // Try cold tier
        if let Some(record) = self.cold.get_by_signature(signature).await? {
            if self.config.auto_promote_on_access {
                let _ = self.hot.put(&record).await;
            }
            return Ok(Some(record));
        }

        Ok(None)
    }

    /// Put a record, initially in hot and warm tiers
    pub async fn put(&self, record: &TransactionRecord) -> Result<i64> {
        // Store in warm tier first (persistent storage)
        let id = self.warm.put(record).await?;

        // Also cache in hot tier
        let mut record_with_id = record.clone();
        record_with_id.id = Some(id);
        self.hot.put(&record_with_id).await?;

        info!("Stored record {} in hot and warm tiers", id);
        Ok(id)
    }

    /// Run automatic tiering maintenance
    /// This should be called periodically to move data between tiers
    pub async fn run_maintenance(&self) -> Result<()> {
        info!("Starting auto-tiering maintenance");

        let now = chrono::Utc::now().timestamp() as u64;

        // Move old data from hot to warm (already in warm, just remove from hot)
        self.evict_old_from_hot(now).await?;

        // Move old data from warm to cold
        self.archive_old_to_cold(now).await?;

        // Run tier-specific maintenance
        self.hot.maintenance().await?;
        self.warm.maintenance().await?;
        self.cold.maintenance().await?;

        // Collect and check metrics
        let metrics = self.collect_metrics().await?;
        self.monitor.check_metrics(&metrics).await;
        self.monitor.log_metrics_summary(&metrics);

        info!("Auto-tiering maintenance completed");
        Ok(())
    }

    /// Evict old records from hot tier
    async fn evict_old_from_hot(&self, now: u64) -> Result<()> {
        // Note: Moka handles LRU eviction automatically
        // This method is for explicit age-based eviction if needed
        debug!("Hot tier eviction handled by LRU policy");
        Ok(())
    }

    /// Archive old records from warm to cold tier
    async fn archive_old_to_cold(&self, now: u64) -> Result<()> {
        let threshold_timestamp = now.saturating_sub(self.config.warm_to_cold_age);

        // Get old records from warm tier
        let old_records = self
            .warm
            .list_since(0)
            .await?
            .into_iter()
            .filter(|r| r.timestamp_decision_made < threshold_timestamp)
            .collect::<Vec<_>>();

        if old_records.is_empty() {
            debug!("No records to archive from warm to cold");
            return Ok(());
        }

        info!(
            "Archiving {} records from warm to cold tier",
            old_records.len()
        );

        // Move records to cold tier
        for record in &old_records {
            if let Some(id) = record.id {
                // Put in cold tier
                self.cold.put(record).await?;

                // Remove from warm tier
                self.warm.remove(id).await?;

                debug!("Archived record {} to cold tier", id);
            }
        }

        info!("Archived {} records to cold tier", old_records.len());
        Ok(())
    }

    /// Collect metrics from all tiers
    pub async fn collect_metrics(&self) -> Result<TierMetrics> {
        Ok(TierMetrics {
            hot: Some(self.hot.get_metrics().await?),
            warm: Some(self.warm.get_metrics().await?),
            cold: Some(self.cold.get_metrics().await?),
        })
    }

    /// Get recent alerts from the monitor
    pub async fn get_alerts(&self) -> Vec<super::metrics::TierAlert> {
        self.monitor.get_alerts().await
    }

    /// Get reference to hot storage (for advanced use cases)
    pub fn hot_tier(&self) -> &HotMemoryStorage {
        &self.hot
    }

    /// Get reference to warm storage (for advanced use cases)
    pub fn warm_tier(&self) -> &WarmSqliteStorage {
        &self.warm
    }

    /// Get reference to cold storage (for advanced use cases)
    pub fn cold_tier(&self) -> &ColdArchiveStorage {
        &self.cold
    }

    /// Start the automatic maintenance loop
    /// This runs in the background and periodically triggers maintenance
    pub async fn start_maintenance_loop(self: Arc<Self>) {
        let interval = Duration::from_secs(self.config.maintenance_interval);

        info!(
            "Starting auto-tiering maintenance loop (interval: {}s)",
            self.config.maintenance_interval
        );

        let mut ticker = time::interval(interval);

        loop {
            ticker.tick().await;

            if let Err(e) = self.run_maintenance().await {
                warn!("Auto-tiering maintenance failed: {}", e);
            }
        }
    }

    /// Get tier statistics
    pub async fn get_stats(&self) -> Result<TierStats> {
        Ok(TierStats {
            hot_count: self.hot.count().await?,
            warm_count: self.warm.count().await?,
            cold_count: self.cold.count().await?,
        })
    }
}

/// Statistics across all tiers
#[derive(Debug, Clone)]
pub struct TierStats {
    pub hot_count: u64,
    pub warm_count: u64,
    pub cold_count: u64,
}

impl TierStats {
    pub fn total_count(&self) -> u64 {
        self.hot_count + self.warm_count + self.cold_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::tiered_storage::metrics::TierMonitor;
    use crate::oracle::types::{Outcome, ScoredCandidate};
    use crate::types::PremintCandidate;
    use rand;
    use std::collections::HashMap;

    fn create_test_record(id: Option<i64>, signature: &str, timestamp: u64) -> TransactionRecord {
        let scored_candidate = ScoredCandidate {
            base: PremintCandidate {
                mint: format!("test_mint_{}", signature),
                creator: "creator".to_string(),
                program: "program".to_string(),
                slot: 0,
                timestamp,
                instruction_summary: None,
                is_jito_bundle: None,
            },
            mint: format!("test_mint_{}", signature),
            predicted_score: 80,
            reason: "test".to_string(),
            feature_scores: HashMap::new(),
            calculation_time: 100,
            anomaly_detected: false,
            timestamp,
        };

        TransactionRecord {
            id,
            scored_candidate,
            transaction_signature: Some(signature.to_string()),
            buy_price_sol: Some(0.001),
            sell_price_sol: None,
            amount_bought_tokens: Some(1000.0),
            amount_sold_tokens: None,
            initial_sol_spent: Some(1.0),
            final_sol_received: None,
            timestamp_decision_made: timestamp,
            timestamp_transaction_sent: None,
            timestamp_outcome_evaluated: None,
            actual_outcome: Outcome::PendingConfirmation,
            market_context_snapshot: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_coordinator_put_and_get() {
        let coordinator = AutoTieringCoordinator::with_test_defaults().await.unwrap();

        let record = create_test_record(None, "coord_sig123", 1000);

        // Put record
        let id = coordinator.put(&record).await.unwrap();
        assert!(id > 0);

        // Get by ID (should be in hot tier)
        let retrieved = coordinator.get(id).await.unwrap();
        assert!(retrieved.is_some());

        // Get by signature
        let retrieved = coordinator.get_by_signature("coord_sig123").await.unwrap();
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_coordinator_auto_promotion() {
        let mut config = TieringConfig::default();
        config.auto_promote_on_access = true;

        let hot = Arc::new(HotMemoryStorage::with_default_capacity());
        let warm = Arc::new(WarmSqliteStorage::with_test_defaults().await.unwrap());
        let cold_path = format!("/tmp/cold_test_{}", rand::random::<u64>());
        let cold = Arc::new(ColdArchiveStorage::new(&cold_path).await.unwrap());
        let monitor = Arc::new(TierMonitor::new());

        let coordinator = AutoTieringCoordinator {
            hot,
            warm,
            cold,
            monitor,
            config,
        };

        let record = create_test_record(Some(1), "promo_sig", 1000);

        // Put directly in warm tier
        coordinator.warm.put(&record).await.unwrap();

        // Access via coordinator (should promote to hot)
        let _ = coordinator.get(1).await.unwrap();

        // Verify it's now in hot tier
        let in_hot = coordinator.hot.contains(1).await.unwrap();
        assert!(in_hot, "Record should be promoted to hot tier");
    }

    #[tokio::test]
    async fn test_coordinator_collect_metrics() {
        let coordinator = AutoTieringCoordinator::with_test_defaults().await.unwrap();

        // Add some test data
        for i in 1..=5 {
            let record = create_test_record(None, &format!("metric_sig_{}", i), 1000 + i);
            coordinator.put(&record).await.unwrap();
        }

        // Wait for caches to process
        coordinator.hot.sync_cache().await;

        let metrics = coordinator.collect_metrics().await.unwrap();

        assert!(metrics.hot.is_some());
        assert!(metrics.warm.is_some());
        assert!(metrics.cold.is_some());

        let hot = metrics.hot.unwrap();
        // Since moka uses async operations, we just check it's non-zero
        assert!(hot.record_count > 0, "Hot tier should have records");
    }

    #[tokio::test]
    async fn test_coordinator_stats() {
        let coordinator = AutoTieringCoordinator::with_test_defaults().await.unwrap();

        // Add test data
        for i in 1..=3 {
            let record = create_test_record(None, &format!("stats_sig_{}", i), 1000 + i);
            coordinator.put(&record).await.unwrap();
        }

        // Wait for caches to process
        coordinator.hot.sync_cache().await;

        let stats = coordinator.get_stats().await.unwrap();

        // Check that we have records in warm tier (persistent)
        assert!(
            stats.warm_count >= 3,
            "Expected at least 3 records in warm tier, got {}",
            stats.warm_count
        );
        // Hot tier may have fewer due to async operations
        assert!(stats.hot_count > 0, "Hot tier should have some records");
    }
}
