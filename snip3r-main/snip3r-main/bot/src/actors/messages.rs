//! Message types for the actor system
//!
//! This module defines all message types used for communication between actors.

use crate::oracle::quantum_oracle::ScoredCandidate;
use crate::oracle::transaction_monitor::MonitoredTransaction;
use crate::oracle::types::{Outcome, TransactionRecord};
use crate::types::PremintCandidate;
use actix::prelude::*;

// ============================================================================
// Oracle Actor Messages
// ============================================================================

/// Message to score a candidate
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct ScoreCandidate {
    pub candidate: PremintCandidate,
}

/// Message to update oracle configuration
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct UpdateOracleConfig {
    pub weights: Option<crate::oracle::types::FeatureWeights>,
    pub thresholds: Option<crate::oracle::types::ScoreThresholds>,
}

/// Message to get oracle metrics
#[derive(Message, Debug, Clone)]
#[rtype(result = "OracleMetrics")]
pub struct GetOracleMetrics;

/// Oracle metrics response
#[derive(Debug, Clone)]
pub struct OracleMetrics {
    pub total_scored: u64,
    pub avg_scoring_time: f64,
    pub high_score_count: u64,
}

// ============================================================================
// Storage Actor Messages (DecisionLedger)
// ============================================================================

/// Message to record a transaction decision
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct RecordDecision {
    pub record: TransactionRecord,
}

/// Message to update transaction outcome
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateOutcome {
    pub signature: String,
    pub outcome: Outcome,
    pub buy_price: Option<f64>,
    pub sell_price: Option<f64>,
    pub sol_spent: Option<f64>,
    pub sol_received: Option<f64>,
    pub evaluated_at: Option<u64>,
    pub is_verified: bool,
}

/// Message to query storage statistics
#[derive(Message, Debug, Clone)]
#[rtype(result = "StorageStats")]
pub struct GetStorageStats;

/// Storage statistics response
#[derive(Debug, Clone)]
pub struct StorageStats {
    pub total_decisions: u64,
    pub pending_outcomes: u64,
}

// ============================================================================
// Monitor Actor Messages (TransactionMonitor)
// ============================================================================

/// Message to add a transaction to monitoring
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct MonitorTransaction {
    pub transaction: MonitoredTransaction,
}

/// Message to get monitoring statistics
#[derive(Message, Debug, Clone)]
#[rtype(result = "MonitorStats")]
pub struct GetMonitorStats;

/// Monitor statistics response
#[derive(Debug, Clone)]
pub struct MonitorStats {
    pub active_transactions: usize,
    pub completed_transactions: u64,
}

// ============================================================================
// Supervisor Actor Messages
// ============================================================================

/// Message to get system health status
#[derive(Message, Debug, Clone)]
#[rtype(result = "SystemHealth")]
pub struct GetSystemHealth;

/// System health response
#[derive(Debug, Clone)]
pub struct SystemHealth {
    pub oracle_healthy: bool,
    pub storage_healthy: bool,
    pub monitor_healthy: bool,
    pub uptime_secs: u64,
}

/// Message to gracefully shutdown the system
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct ShutdownSystem;
