//! Tiered Storage Architecture Module
//!
//! This module implements a three-tier storage system:
//! - Hot Tier (L1): In-memory LRU cache with <1ms access time
//! - Warm Tier (L2): Enhanced SQLite with indexes and pooling, <10ms access time
//! - Cold Tier (L3): Parquet-based archival storage, <100ms access time
//!
//! The system automatically moves data between tiers based on access patterns
//! and age, optimizing for both performance and storage efficiency.

mod cold_storage;
mod coordinator;
mod hot_storage;
mod metrics;
mod traits;
mod warm_storage;

// Re-export main types
pub use cold_storage::ColdArchiveStorage;
pub use coordinator::{AutoTieringCoordinator, TieringConfig};
pub use hot_storage::HotMemoryStorage;
pub use metrics::{TierMetrics, TierMonitor};
pub use traits::StorageTier;
pub use warm_storage::WarmSqliteStorage;
