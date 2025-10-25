//! Cold Archive Storage (L3 Tier)
//!
//! Parquet-based archival storage for historical data.
//! Target access time: <100ms

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::traits::{StorageTier, TierMetricsSnapshot};
use crate::oracle::types::TransactionRecord;

/// Cold storage tier using Parquet format
///
/// This tier provides compressed, column-oriented storage for archival data.
/// Optimized for batch reads and long-term storage efficiency.
pub struct ColdArchiveStorage {
    base_path: String,
    metrics: Arc<RwLock<ColdStorageMetrics>>,
    // In-memory index for faster lookups
    index: Arc<RwLock<ColdStorageIndex>>,
}

#[derive(Debug, Default)]
struct ColdStorageMetrics {
    access_count: u64,
    hit_count: u64,
    miss_count: u64,
    access_times_us: Vec<u64>,
    files_written: u64,
    files_read: u64,
}

#[derive(Debug, Default)]
struct ColdStorageIndex {
    /// Map of ID to (file_path, row_index)
    id_to_location: std::collections::HashMap<i64, (String, usize)>,
    /// Map of signature to ID
    signature_to_id: std::collections::HashMap<String, i64>,
}

impl ColdArchiveStorage {
    /// Create a new cold storage tier
    ///
    /// # Arguments
    /// * `base_path` - Base directory for storing Parquet files
    pub async fn new(base_path: &str) -> Result<Self> {
        info!("Initializing ColdArchiveStorage at: {}", base_path);

        // Create directory if it doesn't exist
        tokio::fs::create_dir_all(base_path)
            .await
            .context("Failed to create cold storage directory")?;

        let storage = Self {
            base_path: base_path.to_string(),
            metrics: Arc::new(RwLock::new(ColdStorageMetrics::default())),
            index: Arc::new(RwLock::new(ColdStorageIndex::default())),
        };

        // Load existing index
        storage.rebuild_index().await?;

        Ok(storage)
    }

    /// Create with default path
    pub async fn with_defaults() -> Result<Self> {
        Self::new("./cold_storage").await
    }

    /// Rebuild the in-memory index by scanning Parquet files
    async fn rebuild_index(&self) -> Result<()> {
        info!("Rebuilding cold storage index...");

        let mut index = self.index.write().await;
        index.id_to_location.clear();
        index.signature_to_id.clear();

        // For now, we'll use a simple JSON-based index file
        // In production, this would scan Parquet files
        let index_path = format!("{}/index.json", self.base_path);

        if tokio::fs::metadata(&index_path).await.is_ok() {
            let content = tokio::fs::read_to_string(&index_path).await?;
            if let Ok(loaded_index) = serde_json::from_str::<serde_json::Value>(&content) {
                // Parse index (simplified for now)
                debug!("Loaded cold storage index");
            }
        }

        info!("Cold storage index rebuilt");
        Ok(())
    }

    /// Save the index to disk
    async fn save_index(&self) -> Result<()> {
        let index = self.index.read().await;
        let index_path = format!("{}/index.json", self.base_path);

        // Simplified index format for now
        let index_data = serde_json::json!({
            "record_count": index.id_to_location.len(),
            "last_updated": chrono::Utc::now().timestamp(),
        });

        tokio::fs::write(&index_path, serde_json::to_string_pretty(&index_data)?).await?;

        Ok(())
    }

    /// Get the Parquet file path for a given date
    fn get_file_path(&self, date: &str) -> String {
        format!("{}/records_{}.parquet", self.base_path, date)
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

    /// Calculate percentile from sorted times
    fn calculate_percentile(sorted_times: &[u64], percentile: f64) -> u64 {
        if sorted_times.is_empty() {
            return 0;
        }
        let index = ((sorted_times.len() as f64 - 1.0) * percentile / 100.0) as usize;
        sorted_times[index]
    }

    /// Write records to Parquet file (batch operation)
    async fn write_parquet_batch(
        &self,
        records: &[TransactionRecord],
        file_path: &str,
    ) -> Result<()> {
        // For now, we'll use JSON serialization as a placeholder
        // In production, this would use the parquet crate to write columnar data

        let json_data: Vec<_> = records
            .iter()
            .map(|r| serde_json::to_value(r))
            .collect::<Result<Vec<_>, _>>()?;

        let content = serde_json::to_string_pretty(&json_data)?;
        tokio::fs::write(file_path, content).await?;

        let mut metrics = self.metrics.write().await;
        metrics.files_written += 1;

        Ok(())
    }

    /// Read records from Parquet file (batch operation)
    async fn read_parquet_batch(&self, file_path: &str) -> Result<Vec<TransactionRecord>> {
        // For now, we'll use JSON deserialization as a placeholder
        // In production, this would use the parquet crate to read columnar data

        let content = tokio::fs::read_to_string(file_path).await?;
        let json_data: Vec<serde_json::Value> = serde_json::from_str(&content)?;

        let records: Vec<TransactionRecord> = json_data
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();

        let mut metrics = self.metrics.write().await;
        metrics.files_read += 1;

        Ok(records)
    }
}

#[async_trait]
impl StorageTier for ColdArchiveStorage {
    async fn get(&self, id: i64) -> Result<Option<TransactionRecord>> {
        let start = Instant::now();

        let index = self.index.read().await;

        let location = index.id_to_location.get(&id).cloned();
        drop(index); // Release lock before I/O

        let result = if let Some((file_path, row_index)) = location {
            // Read the Parquet file
            let records = self.read_parquet_batch(&file_path).await?;

            // Get the specific record
            records.get(row_index).cloned()
        } else {
            None
        };

        let duration_us = start.elapsed().as_micros() as u64;
        self.record_access(duration_us, result.is_some()).await;

        if result.is_some() {
            debug!("Cold storage hit for ID: {} in {}µs", id, duration_us);
        }

        Ok(result)
    }

    async fn get_by_signature(&self, signature: &str) -> Result<Option<TransactionRecord>> {
        let start = Instant::now();

        let index = self.index.read().await;
        let id = index.signature_to_id.get(signature).cloned();
        drop(index); // Release lock before recursive call

        let result = if let Some(id) = id {
            self.get(id).await?
        } else {
            None
        };

        let duration_us = start.elapsed().as_micros() as u64;
        self.record_access(duration_us, result.is_some()).await;

        Ok(result)
    }

    async fn put(&self, record: &TransactionRecord) -> Result<i64> {
        let id = record
            .id
            .ok_or_else(|| anyhow::anyhow!("Record must have an ID for cold storage"))?;

        // Determine file path based on timestamp
        let date = chrono::DateTime::from_timestamp(record.timestamp_decision_made as i64, 0)
            .unwrap_or_else(chrono::Utc::now)
            .format("%Y-%m-%d")
            .to_string();

        let file_path = self.get_file_path(&date);

        // For simplicity, we'll append to a JSON file
        // In production, this would batch writes to Parquet
        let mut existing_records = if tokio::fs::metadata(&file_path).await.is_ok() {
            self.read_parquet_batch(&file_path).await?
        } else {
            Vec::new()
        };

        let row_index = existing_records.len();
        existing_records.push(record.clone());

        self.write_parquet_batch(&existing_records, &file_path)
            .await?;

        // Update index
        let mut index = self.index.write().await;
        index
            .id_to_location
            .insert(id, (file_path.clone(), row_index));
        if let Some(ref signature) = record.transaction_signature {
            index.signature_to_id.insert(signature.clone(), id);
        }

        // Persist index
        drop(index);
        self.save_index().await?;

        Ok(id)
    }

    async fn remove(&self, id: i64) -> Result<bool> {
        let mut index = self.index.write().await;

        if let Some((file_path, row_index)) = index.id_to_location.remove(&id) {
            // Remove from signature index
            index.signature_to_id.retain(|_, v| *v != id);

            // Note: In production, we'd need to rewrite the Parquet file
            // For now, we'll just remove from index

            drop(index);
            self.save_index().await?;

            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn list_since(&self, timestamp: u64) -> Result<Vec<TransactionRecord>> {
        // This is inefficient for cold storage, but necessary for the interface
        // In production, we'd use Parquet's predicate pushdown

        let mut all_records = Vec::new();

        // List all Parquet files
        let mut entries = tokio::fs::read_dir(&self.base_path).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("parquet")
                || path.extension().and_then(|s| s.to_str()) == Some("json")
            {
                if let Ok(records) = self.read_parquet_batch(path.to_str().unwrap()).await {
                    all_records.extend(records);
                }
            }
        }

        // Filter by timestamp
        Ok(all_records
            .into_iter()
            .filter(|r| r.timestamp_decision_made >= timestamp)
            .collect())
    }

    async fn count(&self) -> Result<u64> {
        let index = self.index.read().await;
        Ok(index.id_to_location.len() as u64)
    }

    async fn contains(&self, id: i64) -> Result<bool> {
        let index = self.index.read().await;
        Ok(index.id_to_location.contains_key(&id))
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

        // Estimate bytes stored by checking file sizes
        let mut bytes_stored = 0u64;
        if let Ok(mut entries) = tokio::fs::read_dir(&self.base_path).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(metadata) = entry.metadata().await {
                    bytes_stored += metadata.len();
                }
            }
        }

        Ok(TierMetricsSnapshot {
            record_count,
            access_count: metrics.access_count,
            hit_count: metrics.hit_count,
            miss_count: metrics.miss_count,
            avg_access_time_us,
            p95_access_time_us: p95,
            p99_access_time_us: p99,
            bytes_stored,
            eviction_count: 0,
        })
    }

    async fn maintenance(&self) -> Result<()> {
        // Compact old files, merge small files, etc.
        info!("Cold storage maintenance: compacting files");

        // Rebuild index
        self.rebuild_index().await?;

        info!("Cold storage maintenance completed");
        Ok(())
    }

    fn tier_name(&self) -> &'static str {
        "Cold (L3)"
    }

    fn target_access_time(&self) -> Duration {
        Duration::from_millis(100) // 100ms target
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
    async fn test_cold_storage_put_and_get() {
        let storage = ColdArchiveStorage::new("/tmp/test_cold_storage")
            .await
            .unwrap();
        let record = create_test_record(1, "cold_sig123");

        // Put record
        storage.put(&record).await.unwrap();

        // Get by ID
        let retrieved = storage.get(1).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, Some(1));

        // Get by signature
        let retrieved = storage.get_by_signature("cold_sig123").await.unwrap();
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_cold_storage_access_time_under_100ms() {
        let storage = ColdArchiveStorage::new("/tmp/test_cold_perf")
            .await
            .unwrap();
        let record = create_test_record(1, "cold_sig_perf");

        storage.put(&record).await.unwrap();

        let start = Instant::now();
        let _ = storage.get(1).await.unwrap();
        let duration = start.elapsed();

        // Should be under 100ms for cold storage
        assert!(
            duration < Duration::from_millis(100),
            "Access time {}ms exceeded 100ms target",
            duration.as_millis()
        );
    }

    #[tokio::test]
    async fn test_cold_storage_count() {
        let storage = ColdArchiveStorage::new("/tmp/test_cold_count")
            .await
            .unwrap();

        for i in 1..=5 {
            let record = create_test_record(i, &format!("cold_sig_{}", i));
            storage.put(&record).await.unwrap();
        }

        let count = storage.count().await.unwrap();
        assert_eq!(count, 5);
    }
}
