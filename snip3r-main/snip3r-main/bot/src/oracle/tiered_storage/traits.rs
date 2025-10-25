//! StorageTier trait - Common interface for all storage tiers
//!
//! This trait defines the contract that all storage tiers must implement,
//! allowing seamless data access across Hot, Warm, and Cold storage.

use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;

use crate::oracle::types::TransactionRecord;

/// Common trait for all storage tiers
#[async_trait]
pub trait StorageTier: Send + Sync {
    /// Get a record by its unique identifier
    /// Returns None if not found in this tier
    async fn get(&self, id: i64) -> Result<Option<TransactionRecord>>;

    /// Get a record by transaction signature
    /// Returns None if not found in this tier
    async fn get_by_signature(&self, signature: &str) -> Result<Option<TransactionRecord>>;

    /// Store a record in this tier
    /// Returns the unique identifier assigned to the record
    async fn put(&self, record: &TransactionRecord) -> Result<i64>;

    /// Remove a record from this tier
    /// Used when migrating data between tiers
    async fn remove(&self, id: i64) -> Result<bool>;

    /// Get all records in this tier since a given timestamp
    async fn list_since(&self, timestamp: u64) -> Result<Vec<TransactionRecord>>;

    /// Get the number of records in this tier
    async fn count(&self) -> Result<u64>;

    /// Check if a record exists in this tier
    async fn contains(&self, id: i64) -> Result<bool>;

    /// Get tier-specific metrics (access count, hit rate, etc.)
    async fn get_metrics(&self) -> Result<TierMetricsSnapshot>;

    /// Perform tier-specific maintenance (cleanup, compaction, etc.)
    async fn maintenance(&self) -> Result<()>;

    /// Get the name/type of this tier for logging and monitoring
    fn tier_name(&self) -> &'static str;

    /// Get the target access time for this tier
    fn target_access_time(&self) -> Duration;
}

/// Snapshot of tier-specific metrics
#[derive(Debug, Clone)]
pub struct TierMetricsSnapshot {
    /// Total number of records in this tier
    pub record_count: u64,

    /// Total number of access requests
    pub access_count: u64,

    /// Number of successful hits
    pub hit_count: u64,

    /// Number of cache misses (if applicable)
    pub miss_count: u64,

    /// Average access time in microseconds
    pub avg_access_time_us: u64,

    /// P95 access time in microseconds
    pub p95_access_time_us: u64,

    /// P99 access time in microseconds
    pub p99_access_time_us: u64,

    /// Total bytes stored (estimated)
    pub bytes_stored: u64,

    /// Eviction count (for hot tier)
    pub eviction_count: u64,
}

impl TierMetricsSnapshot {
    /// Calculate hit rate as a percentage
    pub fn hit_rate(&self) -> f64 {
        if self.access_count == 0 {
            0.0
        } else {
            (self.hit_count as f64 / self.access_count as f64) * 100.0
        }
    }

    /// Check if this tier is meeting its performance targets
    pub fn is_within_target(&self, target_us: u64) -> bool {
        self.p95_access_time_us <= target_us
    }
}
