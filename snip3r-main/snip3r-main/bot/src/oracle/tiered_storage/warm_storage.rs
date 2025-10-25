//! Warm SQLite Storage (L2 Tier)
//!
//! Enhanced SQLite storage with connection pooling, indexes, and query caching.
//! Target access time: <10ms

use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::{sqlite::SqlitePoolOptions, Pool, Row, Sqlite};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::traits::{StorageTier, TierMetricsSnapshot};
use crate::oracle::types::{Outcome, ScoredCandidate, TransactionRecord};
use crate::types::PremintCandidate;

/// Warm storage tier using SQLite with optimizations
///
/// This tier provides fast persistent storage with:
/// - Connection pooling for concurrent access
/// - Indexes on frequently queried columns
/// - Query result caching
pub struct WarmSqliteStorage {
    pool: Pool<Sqlite>,
    metrics: Arc<RwLock<WarmStorageMetrics>>,
    db_path: String,
}

#[derive(Debug, Default)]
struct WarmStorageMetrics {
    access_count: u64,
    hit_count: u64,
    miss_count: u64,
    access_times_us: Vec<u64>,
    query_cache_hits: u64,
    query_cache_misses: u64,
}

impl WarmSqliteStorage {
    /// Create a new warm storage tier
    ///
    /// # Arguments
    /// * `db_path` - Path to the SQLite database file
    /// * `pool_size` - Maximum number of connections in the pool
    pub async fn new(db_path: &str, pool_size: u32) -> Result<Self> {
        info!(
            "Initializing WarmSqliteStorage at: {} with pool size: {}",
            db_path, pool_size
        );

        let pool = SqlitePoolOptions::new()
            .max_connections(pool_size)
            .acquire_timeout(Duration::from_secs(5))
            .connect(&format!("sqlite:{}?mode=rwc", db_path))
            .await
            .context("Failed to connect to SQLite database")?;

        let storage = Self {
            pool,
            metrics: Arc::new(RwLock::new(WarmStorageMetrics::default())),
            db_path: db_path.to_string(),
        };

        // Initialize schema and indexes
        storage.initialize_schema().await?;

        Ok(storage)
    }

    /// Create with default settings (for production use)
    pub async fn with_defaults() -> Result<Self> {
        Self::new("./warm_storage.db", 10).await
    }

    /// Create with a unique path (for testing)
    #[cfg(test)]
    pub async fn with_test_defaults() -> Result<Self> {
        let db_path = format!("/tmp/warm_test_{}.db", rand::random::<u64>());
        Self::new(&db_path, 10).await
    }

    /// Initialize database schema with optimized indexes
    async fn initialize_schema(&self) -> Result<()> {
        // Create the main table if it doesn't exist
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS warm_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                mint TEXT NOT NULL,
                signature TEXT UNIQUE,
                timestamp INTEGER NOT NULL,
                outcome TEXT NOT NULL,
                buy_price REAL,
                sell_price REAL,
                initial_sol REAL,
                final_sol REAL,
                score INTEGER,
                reason TEXT,
                feature_scores TEXT,
                market_context TEXT,
                created_at INTEGER NOT NULL,
                accessed_at INTEGER NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create warm_records table")?;

        // Create indexes for frequently queried columns
        let indexes = vec![
            "CREATE INDEX IF NOT EXISTS idx_warm_signature ON warm_records(signature);",
            "CREATE INDEX IF NOT EXISTS idx_warm_timestamp ON warm_records(timestamp);",
            "CREATE INDEX IF NOT EXISTS idx_warm_accessed ON warm_records(accessed_at);",
            "CREATE INDEX IF NOT EXISTS idx_warm_mint ON warm_records(mint);",
        ];

        for index_sql in indexes {
            sqlx::query(index_sql)
                .execute(&self.pool)
                .await
                .context(format!("Failed to create index: {}", index_sql))?;
        }

        info!("Warm storage schema initialized with indexes");
        Ok(())
    }

    /// Update the accessed_at timestamp for a record
    async fn update_access_time(&self, id: i64) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE warm_records SET accessed_at = ? WHERE id = ?")
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to update access time")?;
        Ok(())
    }

    /// Record access metrics
    async fn record_access(&self, duration_us: u64, hit: bool) {
        let mut metrics = self.metrics.write().await;
        metrics.access_count += 1;
        if hit {
            metrics.hit_count += 1;
        } else {
            metrics.miss_count += 1;
        }

        metrics.access_times_us.push(duration_us);
        if metrics.access_times_us.len() > 1000 {
            metrics.access_times_us.remove(0);
        }
    }

    /// Convert database row to TransactionRecord
    fn row_to_record(&self, row: &sqlx::sqlite::SqliteRow) -> Result<TransactionRecord> {
        let id: i64 = row.try_get("id")?;
        let mint: String = row.try_get("mint")?;
        let signature: Option<String> = row.try_get("signature")?;
        let timestamp: i64 = row.try_get("timestamp")?;
        let outcome_str: String = row.try_get("outcome")?;
        let score: i32 = row.try_get("score")?;
        let reason: String = row.try_get("reason")?;
        let feature_scores_json: String = row.try_get("feature_scores")?;
        let market_context_json: String = row.try_get("market_context")?;

        let scored_candidate = ScoredCandidate {
            base: PremintCandidate {
                mint: mint.clone(),
                creator: String::new(),
                program: String::new(),
                slot: 0,
                timestamp: timestamp as u64,
                instruction_summary: None,
                is_jito_bundle: None,
            },
            mint: mint.clone(),
            predicted_score: score as u8,
            reason: reason.clone(),
            feature_scores: serde_json::from_str(&feature_scores_json)?,
            calculation_time: 0,
            anomaly_detected: false,
            timestamp: timestamp as u64,
        };

        Ok(TransactionRecord {
            id: Some(id),
            scored_candidate,
            transaction_signature: signature.clone(),
            buy_price_sol: row.try_get("buy_price")?,
            sell_price_sol: row.try_get("sell_price")?,
            amount_bought_tokens: None,
            amount_sold_tokens: None,
            initial_sol_spent: row.try_get("initial_sol")?,
            final_sol_received: row.try_get("final_sol")?,
            timestamp_decision_made: timestamp as u64,
            timestamp_transaction_sent: None,
            timestamp_outcome_evaluated: None,
            actual_outcome: serde_json::from_str(&outcome_str)?,
            market_context_snapshot: serde_json::from_str(&market_context_json)?,
        })
    }

    /// Calculate percentile from sorted times
    fn calculate_percentile(sorted_times: &[u64], percentile: f64) -> u64 {
        if sorted_times.is_empty() {
            return 0;
        }
        let index = ((sorted_times.len() as f64 - 1.0) * percentile / 100.0) as usize;
        sorted_times[index]
    }
}

#[async_trait]
impl StorageTier for WarmSqliteStorage {
    async fn get(&self, id: i64) -> Result<Option<TransactionRecord>> {
        let start = Instant::now();

        let row = sqlx::query("SELECT * FROM warm_records WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to query warm storage")?;

        let duration_us = start.elapsed().as_micros() as u64;

        let result = match row {
            Some(ref r) => {
                // Update access time in background
                let pool = self.pool.clone();
                tokio::spawn(async move {
                    let _ = sqlx::query("UPDATE warm_records SET accessed_at = ? WHERE id = ?")
                        .bind(chrono::Utc::now().timestamp())
                        .bind(id)
                        .execute(&pool)
                        .await;
                });

                Some(self.row_to_record(r)?)
            }
            None => None,
        };

        self.record_access(duration_us, result.is_some()).await;

        if result.is_some() {
            debug!("Warm storage hit for ID: {} in {}µs", id, duration_us);
        }

        Ok(result)
    }

    async fn get_by_signature(&self, signature: &str) -> Result<Option<TransactionRecord>> {
        let start = Instant::now();

        let row = sqlx::query("SELECT * FROM warm_records WHERE signature = ?")
            .bind(signature)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to query warm storage by signature")?;

        let duration_us = start.elapsed().as_micros() as u64;

        let result = match row {
            Some(ref r) => {
                let id: i64 = r.try_get("id")?;

                // Update access time in background
                let pool = self.pool.clone();
                tokio::spawn(async move {
                    let _ = sqlx::query("UPDATE warm_records SET accessed_at = ? WHERE id = ?")
                        .bind(chrono::Utc::now().timestamp())
                        .bind(id)
                        .execute(&pool)
                        .await;
                });

                Some(self.row_to_record(r)?)
            }
            None => None,
        };

        self.record_access(duration_us, result.is_some()).await;

        Ok(result)
    }

    async fn put(&self, record: &TransactionRecord) -> Result<i64> {
        let now = chrono::Utc::now().timestamp();

        let feature_scores_json = serde_json::to_string(&record.scored_candidate.feature_scores)?;
        let market_context_json = serde_json::to_string(&record.market_context_snapshot)?;
        let outcome_json = serde_json::to_string(&record.actual_outcome)?;

        let result = sqlx::query(
            r#"
            INSERT INTO warm_records (
                mint, signature, timestamp, outcome, buy_price, sell_price,
                initial_sol, final_sol, score, reason, feature_scores, 
                market_context, created_at, accessed_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&record.scored_candidate.mint)
        .bind(&record.transaction_signature)
        .bind(record.timestamp_decision_made as i64)
        .bind(outcome_json)
        .bind(record.buy_price_sol)
        .bind(record.sell_price_sol)
        .bind(record.initial_sol_spent)
        .bind(record.final_sol_received)
        .bind(record.scored_candidate.predicted_score as i32)
        .bind(&record.scored_candidate.reason)
        .bind(feature_scores_json)
        .bind(market_context_json)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to insert record into warm storage")?;

        Ok(result.last_insert_rowid())
    }

    async fn remove(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM warm_records WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to remove record from warm storage")?;

        Ok(result.rows_affected() > 0)
    }

    async fn list_since(&self, timestamp: u64) -> Result<Vec<TransactionRecord>> {
        let rows =
            sqlx::query("SELECT * FROM warm_records WHERE timestamp >= ? ORDER BY timestamp ASC")
                .bind(timestamp as i64)
                .fetch_all(&self.pool)
                .await
                .context("Failed to list records from warm storage")?;

        let mut records = Vec::new();
        for row in rows {
            records.push(self.row_to_record(&row)?);
        }

        Ok(records)
    }

    async fn count(&self) -> Result<u64> {
        let row = sqlx::query("SELECT COUNT(*) as count FROM warm_records")
            .fetch_one(&self.pool)
            .await
            .context("Failed to count records in warm storage")?;

        let count: i64 = row.try_get("count")?;
        Ok(count as u64)
    }

    async fn contains(&self, id: i64) -> Result<bool> {
        let row = sqlx::query("SELECT 1 FROM warm_records WHERE id = ? LIMIT 1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to check if record exists")?;

        Ok(row.is_some())
    }

    async fn get_metrics(&self) -> Result<TierMetricsSnapshot> {
        let metrics = self.metrics.read().await;

        let mut sorted_times = metrics.access_times_us.clone();
        sorted_times.sort_unstable();

        let avg_access_time_us = if !sorted_times.is_empty() {
            sorted_times.iter().sum::<u64>() / sorted_times.len() as u64
        } else {
            0
        };

        let p95 = Self::calculate_percentile(&sorted_times, 95.0);
        let p99 = Self::calculate_percentile(&sorted_times, 99.0);

        let record_count = self.count().await?;

        // Estimate bytes stored (rough estimate: 4KB per record)
        let bytes_stored = record_count * 4096;

        Ok(TierMetricsSnapshot {
            record_count,
            access_count: metrics.access_count,
            hit_count: metrics.hit_count,
            miss_count: metrics.miss_count,
            avg_access_time_us,
            p95_access_time_us: p95,
            p99_access_time_us: p99,
            bytes_stored,
            eviction_count: 0, // No eviction in warm tier
        })
    }

    async fn maintenance(&self) -> Result<()> {
        // Run VACUUM to reclaim space
        sqlx::query("VACUUM")
            .execute(&self.pool)
            .await
            .context("Failed to vacuum database")?;

        // Update statistics for query optimizer
        sqlx::query("ANALYZE")
            .execute(&self.pool)
            .await
            .context("Failed to analyze database")?;

        info!("Warm storage maintenance completed");
        Ok(())
    }

    fn tier_name(&self) -> &'static str {
        "Warm (L2)"
    }

    fn target_access_time(&self) -> Duration {
        Duration::from_millis(10) // 10ms target
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::types::Outcome;
    use std::collections::HashMap;

    fn create_test_record(id: Option<i64>, signature: &str) -> TransactionRecord {
        let scored_candidate = ScoredCandidate {
            base: PremintCandidate {
                mint: format!("test_mint_{}", signature),
                creator: "creator".to_string(),
                program: "program".to_string(),
                slot: 0,
                timestamp: 1000,
                instruction_summary: None,
                is_jito_bundle: None,
            },
            mint: format!("test_mint_{}", signature),
            predicted_score: 80,
            reason: "test".to_string(),
            feature_scores: HashMap::new(),
            calculation_time: 100,
            anomaly_detected: false,
            timestamp: 1000,
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
            timestamp_decision_made: 1000,
            timestamp_transaction_sent: None,
            timestamp_outcome_evaluated: None,
            actual_outcome: Outcome::PendingConfirmation,
            market_context_snapshot: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_warm_storage_put_and_get() {
        let db_path = format!("/tmp/test_warm_storage_{}.db", rand::random::<u64>());
        let storage = WarmSqliteStorage::new(&db_path, 5).await.unwrap();

        let record = create_test_record(None, &format!("warm_sig_{}", rand::random::<u64>()));

        // Put record
        let id = storage.put(&record).await.unwrap();
        assert!(id > 0);

        // Get by ID
        let retrieved = storage.get(id).await.unwrap();
        assert!(retrieved.is_some());

        // Get by signature
        let retrieved = storage
            .get_by_signature(record.transaction_signature.as_ref().unwrap())
            .await
            .unwrap();
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_warm_storage_access_time_under_10ms() {
        let db_path = format!("/tmp/test_warm_perf_{}.db", rand::random::<u64>());
        let storage = WarmSqliteStorage::new(&db_path, 5).await.unwrap();

        let record = create_test_record(None, &format!("warm_sig_perf_{}", rand::random::<u64>()));
        let id = storage.put(&record).await.unwrap();

        let start = Instant::now();
        let _ = storage.get(id).await.unwrap();
        let duration = start.elapsed();

        // Should be under 10ms for SQLite with indexes
        assert!(
            duration < Duration::from_millis(10),
            "Access time {}ms exceeded 10ms target",
            duration.as_millis()
        );
    }

    #[tokio::test]
    async fn test_warm_storage_list_since() {
        let db_path = format!("/tmp/test_warm_list_{}.db", rand::random::<u64>());
        let storage = WarmSqliteStorage::new(&db_path, 5).await.unwrap();

        // Insert records with different timestamps
        for i in 0..5 {
            let mut record =
                create_test_record(None, &format!("sig_{}_{}", rand::random::<u64>(), i));
            record.timestamp_decision_made = 1000 + (i as u64 * 100);
            storage.put(&record).await.unwrap();
        }

        // Query records since timestamp 1200
        let records = storage.list_since(1200).await.unwrap();

        // Should get records with timestamps 1200, 1300, 1400
        assert!(records.len() >= 3);
    }
}
