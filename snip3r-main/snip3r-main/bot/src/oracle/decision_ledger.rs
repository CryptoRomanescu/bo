//! DecisionLedger module - Operational Memory for the Oracle system
//!
//! This module implements the first pillar of the "genius" system: persistent memory
//! of all decisions made by the PredictiveOracle and their actual outcomes.

use crate::oracle::storage::{LedgerStorage, SqliteLedger, SqliteLedgerNormalized};
use crate::oracle::types::{
    DecisionRecordReceiver, Outcome, OutcomeUpdateReceiver, TransactionRecord,
};
use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::{debug, error, info};

/// DecisionLedger provides persistent storage for Oracle decisions and outcomes
/// Now using the storage abstraction for clean separation of concerns
pub struct DecisionLedger {
    storage: Arc<dyn LedgerStorage>,
    record_receiver: DecisionRecordReceiver,
    outcome_update_receiver: OutcomeUpdateReceiver,
}

impl DecisionLedger {
    /// Create a new DecisionLedger with SQLite backend using storage abstraction
    pub async fn new(
        record_receiver: DecisionRecordReceiver,
        outcome_update_receiver: OutcomeUpdateReceiver,
    ) -> Result<Self> {
        let storage = SqliteLedger::new().await?;

        info!("DecisionLedger initialized with flat storage schema");

        Ok(Self {
            storage,
            record_receiver,
            outcome_update_receiver,
        })
    }

    /// Create a new DecisionLedger with normalized SQLite backend
    pub async fn new_normalized(
        record_receiver: DecisionRecordReceiver,
        outcome_update_receiver: OutcomeUpdateReceiver,
    ) -> Result<Self> {
        let storage = SqliteLedgerNormalized::new().await?;

        info!("DecisionLedger initialized with normalized storage schema");

        Ok(Self {
            storage,
            record_receiver,
            outcome_update_receiver,
        })
    }

    /// Create a new DecisionLedger with custom storage implementation
    pub fn new_with_storage(
        storage: Arc<dyn LedgerStorage>,
        record_receiver: DecisionRecordReceiver,
        outcome_update_receiver: OutcomeUpdateReceiver,
    ) -> Self {
        Self {
            storage,
            record_receiver,
            outcome_update_receiver,
        }
    }

    /// Get a reference to the storage for use by other components
    pub fn get_storage(&self) -> Arc<dyn LedgerStorage> {
        Arc::clone(&self.storage)
    }

    /// Get a reference to the database pool for backward compatibility
    /// This method allows existing code to continue working while we migrate
    pub fn get_db_pool(&self) -> Option<&sqlx::Pool<sqlx::Sqlite>> {
        // Try to downcast to SqliteLedger first, then SqliteLedgerNormalized
        if let Some(sqlite_ledger) = self
            .storage
            .as_ref()
            .as_any()
            .downcast_ref::<SqliteLedger>()
        {
            Some(sqlite_ledger.get_db_pool())
        } else if let Some(sqlite_normalized) = self
            .storage
            .as_ref()
            .as_any()
            .downcast_ref::<SqliteLedgerNormalized>()
        {
            Some(sqlite_normalized.get_db_pool())
        } else {
            None
        }
    }

    /// Main execution loop - processes incoming decisions and outcome updates
    #[tracing::instrument(skip(self), name = "decision_ledger_run")]
    pub async fn run(mut self) {
        info!("DecisionLedger is running...");
        loop {
            tokio::select! {
                Some(record) = self.record_receiver.recv() => {
                    if let Err(e) = self.storage.insert_record(&record).await {
                        error!("Failed to insert transaction record: {:?}", e);
                    }
                },
                Some((signature, outcome, buy_price, sell_price, sol_spent, sol_received, evaluated_at, is_verified)) = self.outcome_update_receiver.recv() => {
                    if let Err(e) = self.storage.update_outcome(&signature, outcome, buy_price, sell_price, sol_spent, sol_received, evaluated_at, is_verified).await {
                        error!("Failed to update outcome for signature {}: {:?}", signature, e);
                    }
                },
                else => {
                    info!("DecisionLedger channels closed. Shutting down.");
                    break;
                }
            }
        }
    }

    /// Retrieve historical records since a given timestamp (for analysis)
    pub async fn get_records_since(&self, timestamp: u64) -> Result<Vec<TransactionRecord>> {
        self.storage.get_records_since(timestamp).await
    }
}
