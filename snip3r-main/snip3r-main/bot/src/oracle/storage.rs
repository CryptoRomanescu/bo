//! Storage abstraction layer for the Oracle system
//!
//! This module defines the formal contract for data persistence operations,
//! allowing for clean separation between business logic and storage implementation.

use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::{sqlite::SqlitePoolOptions, FromRow, Pool, Sqlite};
use std::sync::Arc;
use tracing::{debug, info};

use crate::oracle::transaction_monitor::MonitoredTransaction;
use crate::oracle::types::{Outcome, ScoredCandidate, TransactionRecord};

/// Formal contract for persistent operational memory.
/// Defines operations that must be supported by any database engine.
#[async_trait]
pub trait LedgerStorage: Send + Sync {
    /// Saves a new, complete transaction record to the database.
    /// Returns the unique identifier (ID) of the saved record.
    async fn insert_record(&self, record: &TransactionRecord) -> Result<i64>;

    /// Updates the outcome of an existing transaction record identified by signature.
    async fn update_outcome(
        &self,
        signature: &str,
        outcome: Outcome,
        buy_price_sol: Option<f64>,
        sell_price_sol: Option<f64>,
        initial_sol_spent: Option<f64>,
        final_sol_received: Option<f64>,
        timestamp_evaluated: Option<u64>,
        is_verified: bool,
    ) -> Result<()>;

    /// Retrieves historical records since a given timestamp (for analysis).
    /// Returns records ordered by decision timestamp (ascending).
    async fn get_records_since(&self, timestamp: u64) -> Result<Vec<TransactionRecord>>;

    /// Retrieves a specific record by its database ID.
    async fn get_record_by_id(&self, id: i64) -> Result<Option<TransactionRecord>>;

    /// Retrieves records by transaction signature.
    async fn get_record_by_signature(&self, signature: &str) -> Result<Option<TransactionRecord>>;

    /// Gets the total count of records in the storage.
    async fn get_record_count(&self) -> Result<i64>;

    /// Health check for the storage backend.
    async fn health_check(&self) -> Result<bool>;

    // === Persistent Monitoring Queue Methods ===

    /// Enqueue a transaction for monitoring in the persistent queue.
    async fn enqueue_for_monitoring(&self, tx: &MonitoredTransaction) -> Result<()>;

    /// Get all pending transactions from the monitoring queue.
    async fn get_pending_monitoring_transactions(&self) -> Result<Vec<MonitoredTransaction>>;

    /// Update the status of a transaction in the monitoring queue.
    async fn update_monitoring_status(&self, signature: &str, status: &str) -> Result<()>;

    /// Remove completed transactions from the monitoring queue (cleanup).
    async fn cleanup_completed_monitoring(&self) -> Result<u64>;

    /// Allows downcasting to concrete storage types for backward compatibility
    fn as_any(&self) -> &dyn std::any::Any;
}

const DB_FILE: &str = "./decisions.db";

/// Helper type for deserializing records from SQLite
#[derive(FromRow)]
struct TransactionRecordRow {
    id: i64,
    mint: String,
    score: i32,
    reason: String,
    feature_scores: String, // JSON
    calculation_time: i64,
    anomaly_detected: bool,
    timestamp_decision_made: i64,

    transaction_signature: Option<String>,
    buy_price_sol: Option<f64>,
    sell_price_sol: Option<f64>,
    amount_bought_tokens: Option<f64>,
    amount_sold_tokens: Option<f64>,
    initial_sol_spent: Option<f64>,
    final_sol_received: Option<f64>,

    timestamp_transaction_sent: Option<i64>,
    timestamp_outcome_evaluated: Option<i64>,
    actual_outcome: String,          // Enum serialized to string
    market_context_snapshot: String, // JSON
}

/// SQLite implementation of the LedgerStorage trait.
/// This implementation uses the current flat table schema but is designed
/// to be easily replaceable with the normalized schema or PostgreSQL.
pub struct SqliteLedger {
    pool: Pool<Sqlite>,
}

/// Normalized SQLite implementation of the LedgerStorage trait.
/// This implementation uses a normalized schema that separates:
/// - Main trades table
/// - Feature scores in a separate table
/// - Market context in a separate table
/// This design is optimized for analytical queries and future PostgreSQL migration.
pub struct SqliteLedgerNormalized {
    pool: Pool<Sqlite>,
}

impl SqliteLedger {
    /// Creates a new SqliteLedger with the current flat table schema.
    /// This maintains compatibility with the existing system.
    pub async fn new() -> Result<Arc<Self>> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&format!("sqlite:{}?mode=rwc", DB_FILE))
            .await
            .context("Failed to connect to SQLite database")?;

        // Create the transaction_records table if it doesn't exist
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS transaction_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                mint TEXT NOT NULL,
                score INTEGER NOT NULL,
                reason TEXT NOT NULL,
                feature_scores TEXT NOT NULL,
                calculation_time INTEGER NOT NULL,
                anomaly_detected BOOLEAN NOT NULL,
                timestamp_decision_made INTEGER NOT NULL,

                transaction_signature TEXT,
                buy_price_sol REAL,
                sell_price_sol REAL,
                amount_bought_tokens REAL,
                amount_sold_tokens REAL,
                initial_sol_spent REAL,
                final_sol_received REAL,

                timestamp_transaction_sent INTEGER,
                timestamp_outcome_evaluated INTEGER,
                actual_outcome TEXT NOT NULL,
                market_context_snapshot TEXT NOT NULL,
                is_verified BOOLEAN NOT NULL DEFAULT FALSE
            );
            "#,
        )
        .execute(&pool)
        .await
        .context("Failed to create transaction_records table")?;

        // Create the monitoring_queue table for persistent transaction monitoring
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS monitoring_queue (
                signature TEXT PRIMARY KEY NOT NULL,
                mint TEXT NOT NULL,
                initial_sol_spent REAL NOT NULL,
                -- Status: Pending, Processing, Completed, Failed
                status TEXT NOT NULL DEFAULT 'Pending',
                created_at INTEGER NOT NULL
            );
            "#,
        )
        .execute(&pool)
        .await
        .context("Failed to create monitoring_queue table")?;

        info!("SqliteLedger initialized and connected to {}", DB_FILE);

        Ok(Arc::new(Self { pool }))
    }

    /// Get a reference to the database pool for backward compatibility
    pub fn get_db_pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }
}

#[async_trait]
impl LedgerStorage for SqliteLedger {
    async fn insert_record(&self, record: &TransactionRecord) -> Result<i64> {
        debug!(
            "Inserting new transaction record for mint: {}",
            record.scored_candidate.mint
        );

        let feature_scores_json = serde_json::to_string(&record.scored_candidate.feature_scores)?;
        let market_context_json = serde_json::to_string(&record.market_context_snapshot)?;

        let result = sqlx::query(
            r#"
            INSERT INTO transaction_records (
                mint, score, reason, feature_scores, calculation_time, anomaly_detected,
                timestamp_decision_made, transaction_signature, actual_outcome, market_context_snapshot,
                buy_price_sol, sell_price_sol, amount_bought_tokens, amount_sold_tokens,
                initial_sol_spent, final_sol_received, timestamp_transaction_sent, timestamp_outcome_evaluated
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);
            "#
        )
        .bind(record.scored_candidate.mint.clone())
        .bind(record.scored_candidate.predicted_score as i64)
        .bind(record.scored_candidate.reason.clone())
        .bind(feature_scores_json)
        .bind(record.scored_candidate.calculation_time as i64)
        .bind(record.scored_candidate.anomaly_detected)
        .bind(record.timestamp_decision_made as i64)
        .bind(record.transaction_signature.clone())
        .bind(serde_json::to_string(&record.actual_outcome)?) // Serialize Outcome enum
        .bind(market_context_json)
        .bind(record.buy_price_sol)
        .bind(record.sell_price_sol)
        .bind(record.amount_bought_tokens)
        .bind(record.amount_sold_tokens)
        .bind(record.initial_sol_spent)
        .bind(record.final_sol_received)
        .bind(record.timestamp_transaction_sent.map(|t| t as i64))
        .bind(record.timestamp_outcome_evaluated.map(|t| t as i64))
        .execute(&self.pool)
        .await
        .context("Failed to insert record into DB")?;

        Ok(result.last_insert_rowid())
    }

    async fn update_outcome(
        &self,
        signature: &str,
        outcome: Outcome,
        buy_price_sol: Option<f64>,
        sell_price_sol: Option<f64>,
        initial_sol_spent: Option<f64>,
        final_sol_received: Option<f64>,
        timestamp_evaluated: Option<u64>,
        is_verified: bool,
    ) -> Result<()> {
        debug!(
            "Updating outcome for signature: {} (verified: {})",
            signature, is_verified
        );

        sqlx::query(
            r#"
            UPDATE transaction_records
            SET
                actual_outcome = ?,
                buy_price_sol = COALESCE(?, buy_price_sol),
                sell_price_sol = COALESCE(?, sell_price_sol),
                initial_sol_spent = COALESCE(?, initial_sol_spent),
                final_sol_received = COALESCE(?, final_sol_received),
                timestamp_outcome_evaluated = COALESCE(?, timestamp_outcome_evaluated),
                is_verified = ?
            WHERE transaction_signature = ?;
            "#,
        )
        .bind(serde_json::to_string(&outcome)?)
        .bind(buy_price_sol)
        .bind(sell_price_sol)
        .bind(initial_sol_spent)
        .bind(final_sol_received)
        .bind(timestamp_evaluated.map(|t| t as i64))
        .bind(is_verified)
        .bind(signature)
        .execute(&self.pool)
        .await
        .context(format!(
            "Failed to update outcome for signature {}",
            signature
        ))?;

        Ok(())
    }

    async fn get_records_since(&self, timestamp: u64) -> Result<Vec<TransactionRecord>> {
        let rows: Vec<TransactionRecordRow> = sqlx::query_as(
            r#"
            SELECT * FROM transaction_records 
            WHERE timestamp_decision_made >= ? 
            ORDER BY timestamp_decision_made ASC;
            "#,
        )
        .bind(timestamp as i64)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch historical records")?;

        let mut records = Vec::new();
        for row in rows {
            records.push(self.row_to_transaction_record(row)?);
        }

        Ok(records)
    }

    async fn get_record_by_id(&self, id: i64) -> Result<Option<TransactionRecord>> {
        let row: Option<TransactionRecordRow> =
            sqlx::query_as("SELECT * FROM transaction_records WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .context("Failed to fetch record by ID")?;

        match row {
            Some(row) => Ok(Some(self.row_to_transaction_record(row)?)),
            None => Ok(None),
        }
    }

    async fn get_record_by_signature(&self, signature: &str) -> Result<Option<TransactionRecord>> {
        let row: Option<TransactionRecordRow> =
            sqlx::query_as("SELECT * FROM transaction_records WHERE transaction_signature = ?")
                .bind(signature)
                .fetch_optional(&self.pool)
                .await
                .context("Failed to fetch record by signature")?;

        match row {
            Some(row) => Ok(Some(self.row_to_transaction_record(row)?)),
            None => Ok(None),
        }
    }

    async fn get_record_count(&self) -> Result<i64> {
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM transaction_records")
            .fetch_one(&self.pool)
            .await
            .context("Failed to get record count")?;

        Ok(count.0)
    }

    async fn health_check(&self) -> Result<bool> {
        match sqlx::query("SELECT 1").execute(&self.pool).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    // === Persistent Monitoring Queue Implementation ===

    async fn enqueue_for_monitoring(&self, tx: &MonitoredTransaction) -> Result<()> {
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO monitoring_queue (signature, mint, initial_sol_spent, status, created_at)
            VALUES (?, ?, ?, ?, ?)
            "#
        )
        .bind(&tx.signature)
        .bind(tx.mint.clone())
        .bind(tx.initial_sol_spent)
        .bind("Pending")
        .bind(chrono::Utc::now().timestamp_millis())
        .execute(&self.pool)
        .await
        .context("Failed to enqueue transaction for monitoring")?;

        debug!("Enqueued transaction {} for monitoring", tx.signature);
        Ok(())
    }

    async fn get_pending_monitoring_transactions(&self) -> Result<Vec<MonitoredTransaction>> {
        let rows: Vec<(String, String, f64, i64)> = sqlx::query_as(
            r#"
            SELECT signature, mint, initial_sol_spent, created_at 
            FROM monitoring_queue 
            WHERE status = 'Pending' 
            ORDER BY created_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch pending monitoring transactions")?;

        let mut transactions = Vec::new();
        for (signature, mint, initial_sol_spent, created_at) in rows {
            // Calculate monitor_until as 30 seconds after creation (as per current TransactionMonitor logic)
            let monitor_until = (created_at + 30_000) as u64; // 30 seconds in milliseconds

            transactions.push(MonitoredTransaction {
                signature,
                mint: mint.clone(), // The mint field in DB is stored as String, same as Pubkey type alias
                amount_bought_tokens: 0.0, // Not stored in queue, will be reconstructed if needed
                initial_sol_spent,
                monitor_until,
            });
        }

        Ok(transactions)
    }

    async fn update_monitoring_status(&self, signature: &str, status: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE monitoring_queue 
            SET status = ? 
            WHERE signature = ?
            "#,
        )
        .bind(status)
        .bind(signature)
        .execute(&self.pool)
        .await
        .context("Failed to update monitoring status")?;

        debug!(
            "Updated monitoring status for transaction {} to {}",
            signature, status
        );
        Ok(())
    }

    async fn cleanup_completed_monitoring(&self) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM monitoring_queue 
            WHERE status IN ('Completed', 'Failed')
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to cleanup completed monitoring transactions")?;

        let rows_affected = result.rows_affected();
        if rows_affected > 0 {
            debug!(
                "Cleaned up {} completed monitoring transactions",
                rows_affected
            );
        }
        Ok(rows_affected)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl SqliteLedger {
    /// Helper method to convert a database row to a TransactionRecord
    fn row_to_transaction_record(&self, row: TransactionRecordRow) -> Result<TransactionRecord> {
        // Reconstruct the TransactionRecord from the database row
        let scored_candidate = ScoredCandidate {
            base: crate::types::PremintCandidate {
                mint: row.mint.clone(),
                creator: String::new(), // TODO: Store base candidate as JSON
                program: String::new(),
                slot: 0,
                timestamp: row.timestamp_decision_made as u64,
                instruction_summary: None,
                is_jito_bundle: None,
            },
            mint: row.mint.clone(),
            predicted_score: row.score as u8,
            reason: row.reason,
            feature_scores: serde_json::from_str(&row.feature_scores)?,
            calculation_time: row.calculation_time as u128,
            anomaly_detected: row.anomaly_detected,
            timestamp: row.timestamp_decision_made as u64,
        };

        Ok(TransactionRecord {
            id: Some(row.id),
            scored_candidate,
            transaction_signature: row.transaction_signature,
            buy_price_sol: row.buy_price_sol,
            sell_price_sol: row.sell_price_sol,
            amount_bought_tokens: row.amount_bought_tokens,
            amount_sold_tokens: row.amount_sold_tokens,
            initial_sol_spent: row.initial_sol_spent,
            final_sol_received: row.final_sol_received,
            timestamp_decision_made: row.timestamp_decision_made as u64,
            timestamp_transaction_sent: row.timestamp_transaction_sent.map(|t| t as u64),
            timestamp_outcome_evaluated: row.timestamp_outcome_evaluated.map(|t| t as u64),
            actual_outcome: serde_json::from_str(&row.actual_outcome)?,
            market_context_snapshot: serde_json::from_str(&row.market_context_snapshot)?,
        })
    }
}

impl SqliteLedgerNormalized {
    /// Creates a new SqliteLedgerNormalized with the normalized table schema.
    /// This is the new, optimized schema designed for analytical queries.
    pub async fn new() -> Result<Arc<Self>> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&format!("sqlite:{}?mode=rwc", DB_FILE))
            .await
            .context("Failed to connect to SQLite database")?;

        // Create the normalized schema tables
        // Main trades table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS trades (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                mint TEXT NOT NULL,
                decision_timestamp INTEGER NOT NULL,
                signature TEXT UNIQUE,
                final_outcome TEXT,
                pnl_sol REAL,
                is_verified BOOLEAN NOT NULL DEFAULT FALSE
            );
            "#,
        )
        .execute(&pool)
        .await
        .context("Failed to create trades table")?;

        // Table for storing feature values at decision time
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS decision_features (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                trade_id INTEGER NOT NULL,
                feature_name TEXT NOT NULL,
                feature_value REAL NOT NULL,
                FOREIGN KEY (trade_id) REFERENCES trades (id)
            );
            "#,
        )
        .execute(&pool)
        .await
        .context("Failed to create decision_features table")?;

        // Table for storing market context at decision time
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS market_context (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                trade_id INTEGER NOT NULL,
                metric_name TEXT NOT NULL,
                metric_value REAL NOT NULL,
                FOREIGN KEY (trade_id) REFERENCES trades (id)
            );
            "#,
        )
        .execute(&pool)
        .await
        .context("Failed to create market_context table")?;

        // Create the monitoring_queue table for persistent transaction monitoring
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS monitoring_queue (
                signature TEXT PRIMARY KEY NOT NULL,
                mint TEXT NOT NULL,
                initial_sol_spent REAL NOT NULL,
                -- Status: Pending, Processing, Completed, Failed
                status TEXT NOT NULL DEFAULT 'Pending',
                created_at INTEGER NOT NULL
            );
            "#,
        )
        .execute(&pool)
        .await
        .context("Failed to create monitoring_queue table")?;

        info!("SqliteLedgerNormalized initialized with normalized schema");

        Ok(Arc::new(Self { pool }))
    }

    /// Get a reference to the database pool
    pub fn get_db_pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }
}

#[async_trait]
impl LedgerStorage for SqliteLedgerNormalized {
    async fn insert_record(&self, record: &TransactionRecord) -> Result<i64> {
        debug!(
            "Inserting new transaction record for mint: {} (normalized schema)",
            record.scored_candidate.mint
        );

        // Start a transaction to ensure atomicity
        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin transaction")?;

        // 1. Insert into trades table and get the ID
        let trade_id = sqlx::query(
            r#"
            INSERT INTO trades (mint, decision_timestamp, signature, final_outcome, pnl_sol, is_verified)
            VALUES (?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(&record.scored_candidate.mint)
        .bind(record.timestamp_decision_made as i64)
        .bind(&record.transaction_signature)
        .bind(serde_json::to_string(&record.actual_outcome)?)
        .bind(record.final_sol_received.map(|received| {
            received - record.initial_sol_spent.unwrap_or(0.0)
        }))
        .bind(record.actual_outcome != Outcome::NotExecuted && record.actual_outcome != Outcome::PendingConfirmation)
        .execute(&mut *tx)
        .await
        .context("Failed to insert into trades table")?
        .last_insert_rowid();

        // 2. Insert all feature scores into decision_features table
        for (feature_name, feature_value) in &record.scored_candidate.feature_scores {
            sqlx::query(
                "INSERT INTO decision_features (trade_id, feature_name, feature_value) VALUES (?, ?, ?)"
            )
            .bind(trade_id)
            .bind(feature_name)
            .bind(feature_value)
            .execute(&mut *tx)
            .await
            .context("Failed to insert feature score")?;
        }

        // 3. Insert market context into market_context table
        for (metric_name, metric_value) in &record.market_context_snapshot {
            sqlx::query(
                "INSERT INTO market_context (trade_id, metric_name, metric_value) VALUES (?, ?, ?)",
            )
            .bind(trade_id)
            .bind(metric_name)
            .bind(metric_value)
            .execute(&mut *tx)
            .await
            .context("Failed to insert market context")?;
        }

        // Commit the transaction
        tx.commit().await.context("Failed to commit transaction")?;

        Ok(trade_id)
    }

    async fn update_outcome(
        &self,
        signature: &str,
        outcome: Outcome,
        buy_price_sol: Option<f64>,
        sell_price_sol: Option<f64>,
        initial_sol_spent: Option<f64>,
        final_sol_received: Option<f64>,
        timestamp_evaluated: Option<u64>,
        is_verified: bool,
    ) -> Result<()> {
        debug!(
            "Updating outcome for signature: {} (normalized schema, verified: {})",
            signature, is_verified
        );

        let pnl_sol = match (final_sol_received, initial_sol_spent) {
            (Some(received), Some(spent)) => Some(received - spent),
            _ => None,
        };

        sqlx::query(
            r#"
            UPDATE trades
            SET
                final_outcome = ?,
                pnl_sol = COALESCE(?, pnl_sol),
                is_verified = ?
            WHERE signature = ?
            "#,
        )
        .bind(serde_json::to_string(&outcome)?)
        .bind(pnl_sol)
        .bind(is_verified)
        .bind(signature)
        .execute(&self.pool)
        .await
        .context(format!(
            "Failed to update outcome for signature {}",
            signature
        ))?;

        Ok(())
    }

    async fn get_records_since(&self, timestamp: u64) -> Result<Vec<TransactionRecord>> {
        // This is a complex query that joins all three tables to reconstruct the records
        #[derive(sqlx::FromRow)]
        struct TradeRow {
            id: i64,
            mint: String,
            decision_timestamp: i64,
            signature: Option<String>,
            final_outcome: Option<String>,
            pnl_sol: Option<f64>,
            is_verified: bool,
        }

        let trade_rows: Vec<TradeRow> = sqlx::query_as(
            r#"
            SELECT id, mint, decision_timestamp, signature, final_outcome, pnl_sol, is_verified
            FROM trades 
            WHERE decision_timestamp >= ? 
            ORDER BY decision_timestamp ASC
            "#,
        )
        .bind(timestamp as i64)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch trade records")?;

        let mut records = Vec::new();

        for trade_row in trade_rows {
            // Get feature scores for this trade
            #[derive(sqlx::FromRow)]
            struct FeatureRow {
                feature_name: String,
                feature_value: f64,
            }

            let feature_rows: Vec<FeatureRow> = sqlx::query_as(
                "SELECT feature_name, feature_value FROM decision_features WHERE trade_id = ?",
            )
            .bind(trade_row.id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch feature scores")?;

            let mut feature_scores = std::collections::HashMap::new();
            for feature_row in feature_rows {
                feature_scores.insert(feature_row.feature_name, feature_row.feature_value);
            }

            // Get market context for this trade
            #[derive(sqlx::FromRow)]
            struct ContextRow {
                metric_name: String,
                metric_value: f64,
            }

            let context_rows: Vec<ContextRow> = sqlx::query_as(
                "SELECT metric_name, metric_value FROM market_context WHERE trade_id = ?",
            )
            .bind(trade_row.id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch market context")?;

            let mut market_context_snapshot = std::collections::HashMap::new();
            for context_row in context_rows {
                market_context_snapshot.insert(context_row.metric_name, context_row.metric_value);
            }

            // Reconstruct the TransactionRecord
            let scored_candidate = ScoredCandidate {
                base: crate::types::PremintCandidate {
                    mint: trade_row.mint.clone(),
                    creator: String::new(), // TODO: Store in separate table
                    program: String::new(),
                    slot: 0,
                    timestamp: trade_row.decision_timestamp as u64,
                    instruction_summary: None,
                    is_jito_bundle: None,
                },
                mint: trade_row.mint.clone(),
                predicted_score: 0,    // TODO: Store in trades table
                reason: String::new(), // TODO: Store in trades table
                feature_scores,
                calculation_time: 0,     // TODO: Store in trades table
                anomaly_detected: false, // TODO: Store in trades table
                timestamp: trade_row.decision_timestamp as u64,
            };

            records.push(TransactionRecord {
                id: Some(trade_row.id),
                scored_candidate,
                transaction_signature: trade_row.signature,
                buy_price_sol: None,        // TODO: Store in trades table
                sell_price_sol: None,       // TODO: Store in trades table
                amount_bought_tokens: None, // TODO: Store in trades table
                amount_sold_tokens: None,   // TODO: Store in trades table
                initial_sol_spent: trade_row
                    .pnl_sol
                    .map(|pnl| if pnl < 0.0 { -pnl } else { 0.0 }),
                final_sol_received: trade_row
                    .pnl_sol
                    .map(|pnl| if pnl > 0.0 { pnl } else { 0.0 }),
                timestamp_decision_made: trade_row.decision_timestamp as u64,
                timestamp_transaction_sent: None, // TODO: Store in trades table
                timestamp_outcome_evaluated: None, // TODO: Store in trades table
                actual_outcome: serde_json::from_str(
                    &trade_row
                        .final_outcome
                        .unwrap_or_else(|| serde_json::to_string(&Outcome::NotExecuted).unwrap()),
                )?,
                market_context_snapshot,
            });
        }

        Ok(records)
    }

    async fn get_record_by_id(&self, id: i64) -> Result<Option<TransactionRecord>> {
        // For simplicity, delegate to get_records_since and filter
        let all_records = self.get_records_since(0).await?;
        Ok(all_records.into_iter().find(|r| r.id == Some(id)))
    }

    async fn get_record_by_signature(&self, signature: &str) -> Result<Option<TransactionRecord>> {
        // For simplicity, delegate to get_records_since and filter
        let all_records = self.get_records_since(0).await?;
        Ok(all_records
            .into_iter()
            .find(|r| r.transaction_signature.as_ref().map(|s| s.as_str()) == Some(signature)))
    }

    async fn get_record_count(&self) -> Result<i64> {
        #[derive(sqlx::FromRow)]
        struct CountRow {
            count: i64,
        }

        let count: CountRow = sqlx::query_as("SELECT COUNT(*) as count FROM trades")
            .fetch_one(&self.pool)
            .await
            .context("Failed to get record count")?;

        Ok(count.count)
    }

    async fn health_check(&self) -> Result<bool> {
        match sqlx::query("SELECT 1").execute(&self.pool).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    // === Persistent Monitoring Queue Implementation ===

    async fn enqueue_for_monitoring(&self, tx: &MonitoredTransaction) -> Result<()> {
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO monitoring_queue (signature, mint, initial_sol_spent, status, created_at)
            VALUES (?, ?, ?, ?, ?)
            "#
        )
        .bind(&tx.signature)
        .bind(tx.mint.clone())
        .bind(tx.initial_sol_spent)
        .bind("Pending")
        .bind(chrono::Utc::now().timestamp_millis())
        .execute(&self.pool)
        .await
        .context("Failed to enqueue transaction for monitoring")?;

        debug!("Enqueued transaction {} for monitoring", tx.signature);
        Ok(())
    }

    async fn get_pending_monitoring_transactions(&self) -> Result<Vec<MonitoredTransaction>> {
        let rows: Vec<(String, String, f64, i64)> = sqlx::query_as(
            r#"
            SELECT signature, mint, initial_sol_spent, created_at 
            FROM monitoring_queue 
            WHERE status = 'Pending' 
            ORDER BY created_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch pending monitoring transactions")?;

        let mut transactions = Vec::new();
        for (signature, mint, initial_sol_spent, created_at) in rows {
            // Calculate monitor_until as 30 seconds after creation (as per current TransactionMonitor logic)
            let monitor_until = (created_at + 30_000) as u64; // 30 seconds in milliseconds

            transactions.push(MonitoredTransaction {
                signature,
                mint: mint.clone(), // The mint field in DB is stored as String, same as Pubkey type alias
                amount_bought_tokens: 0.0, // Not stored in queue, will be reconstructed if needed
                initial_sol_spent,
                monitor_until,
            });
        }

        Ok(transactions)
    }

    async fn update_monitoring_status(&self, signature: &str, status: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE monitoring_queue 
            SET status = ? 
            WHERE signature = ?
            "#,
        )
        .bind(status)
        .bind(signature)
        .execute(&self.pool)
        .await
        .context("Failed to update monitoring status")?;

        debug!(
            "Updated monitoring status for transaction {} to {}",
            signature, status
        );
        Ok(())
    }

    async fn cleanup_completed_monitoring(&self) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM monitoring_queue 
            WHERE status IN ('Completed', 'Failed')
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to cleanup completed monitoring transactions")?;

        let rows_affected = result.rows_affected();
        if rows_affected > 0 {
            debug!(
                "Cleaned up {} completed monitoring transactions",
                rows_affected
            );
        }
        Ok(rows_affected)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
