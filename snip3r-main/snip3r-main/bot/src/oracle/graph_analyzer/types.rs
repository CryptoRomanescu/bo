//! Types for on-chain graph analysis
//!
//! This module defines the core data structures for building and analyzing
//! transaction graphs on Solana blockchain.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a wallet address in the transaction graph
pub type WalletAddress = String;

/// Represents a transaction signature
pub type TxSignature = String;

/// Transaction edge in the graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionEdge {
    /// Transaction signature
    pub signature: TxSignature,
    /// Amount transferred (in lamports for SOL, or token amount)
    pub amount: u64,
    /// Timestamp of transaction
    pub timestamp: DateTime<Utc>,
    /// Transaction slot
    pub slot: u64,
    /// Token mint address (None for SOL transfers)
    pub token_mint: Option<String>,
}

/// Node statistics for pattern detection
#[derive(Debug, Clone, Default)]
pub struct NodeStats {
    /// Total incoming transaction count
    pub in_degree: usize,
    /// Total outgoing transaction count
    pub out_degree: usize,
    /// Total volume received
    pub total_received: u64,
    /// Total volume sent
    pub total_sent: u64,
    /// First transaction timestamp
    pub first_seen: Option<DateTime<Utc>>,
    /// Last transaction timestamp
    pub last_seen: Option<DateTime<Utc>>,
    /// Unique counter-parties
    pub unique_counterparties: usize,
}

/// Detected pattern type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatternType {
    /// Pump and dump scheme
    PumpAndDump,
    /// Wash trading
    WashTrading,
    /// Coordinated buying
    CoordinatedBuying,
    /// Sybil cluster
    SybilCluster,
    /// Normal trading
    Normal,
}

/// Pattern detection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphPatternMatch {
    /// Type of pattern detected
    pub pattern_type: PatternType,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f64,
    /// Involved wallet addresses
    pub involved_wallets: Vec<WalletAddress>,
    /// Supporting evidence
    pub evidence: PatternEvidence,
}

/// Evidence supporting pattern detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternEvidence {
    /// Temporal clustering score (transactions happening in short time windows)
    pub temporal_clustering: f64,
    /// Circular trading score (A->B->C->A patterns)
    pub circular_trading: f64,
    /// Volume spike score
    pub volume_spike: f64,
    /// Price manipulation score
    pub price_manipulation: f64,
    /// Wallet age suspicion score
    pub wallet_age_suspicion: f64,
    /// Additional notes
    pub notes: Vec<String>,
}

/// Wallet cluster - group of related wallets
#[derive(Debug, Clone)]
pub struct WalletCluster {
    /// Cluster ID
    pub id: usize,
    /// Wallets in this cluster
    pub wallets: Vec<WalletAddress>,
    /// Cluster cohesion score (0.0 - 1.0)
    pub cohesion: f64,
    /// Average time between transactions in cluster
    pub avg_time_between_txs: f64,
    /// Detected pattern (if any)
    pub pattern: Option<PatternType>,
}

/// Graph analysis metrics
#[derive(Debug, Clone, Default)]
pub struct GraphMetrics {
    /// Total number of nodes (wallets)
    pub node_count: usize,
    /// Total number of edges (transactions)
    pub edge_count: usize,
    /// Number of connected components
    pub connected_components: usize,
    /// Number of detected patterns
    pub detected_patterns: HashMap<PatternType, usize>,
    /// Graph density (edges / possible_edges)
    pub density: f64,
    /// Average clustering coefficient
    pub avg_clustering_coefficient: f64,
    /// Build time in milliseconds
    pub build_time_ms: u64,
    /// Memory usage in bytes
    pub memory_usage_bytes: usize,
}

impl GraphMetrics {
    /// Create new empty metrics
    pub fn new() -> Self {
        Self::default()
    }
}

/// Configuration for graph analyzer
#[derive(Debug, Clone)]
pub struct GraphAnalyzerConfig {
    /// Maximum number of transactions to analyze per token
    pub max_transactions: usize,
    /// Time window for pattern detection (in seconds)
    pub time_window_secs: i64,
    /// Minimum confidence threshold for pattern detection
    pub min_confidence: f64,
    /// Maximum wallets to track in graph
    pub max_wallets: usize,
    /// Enable real-time updates
    pub enable_realtime_updates: bool,
    /// Pump detection threshold (price increase ratio)
    pub pump_threshold: f64,
    /// Wash trading detection threshold (circular tx ratio)
    pub wash_trading_threshold: f64,
}

impl Default for GraphAnalyzerConfig {
    fn default() -> Self {
        Self {
            max_transactions: 10_000,
            time_window_secs: 3600, // 1 hour
            min_confidence: 0.7,
            max_wallets: 10_000,
            enable_realtime_updates: true,
            pump_threshold: 5.0,         // 5x price increase
            wash_trading_threshold: 0.3, // 30% circular transactions
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = GraphAnalyzerConfig::default();
        assert_eq!(config.max_wallets, 10_000);
        assert!(config.enable_realtime_updates);
        assert_eq!(config.min_confidence, 0.7);
    }

    #[test]
    fn test_graph_metrics_creation() {
        let metrics = GraphMetrics::new();
        assert_eq!(metrics.node_count, 0);
        assert_eq!(metrics.edge_count, 0);
    }

    #[test]
    fn test_pattern_type_equality() {
        assert_eq!(PatternType::PumpAndDump, PatternType::PumpAndDump);
        assert_ne!(PatternType::PumpAndDump, PatternType::WashTrading);
    }
}
