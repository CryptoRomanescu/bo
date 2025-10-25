//! Transaction Graph Builder
//!
//! Constructs a directed graph from Solana transaction data
//! with efficient incremental updates.

use super::types::*;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use petgraph::graph::{DiGraph, NodeIndex};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::UiTransactionEncoding;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

/// Transaction graph structure
pub struct TransactionGraph {
    /// The underlying directed graph
    graph: DiGraph<WalletAddress, TransactionEdge>,
    /// Map from wallet address to node index
    wallet_to_node: HashMap<WalletAddress, NodeIndex>,
    /// Node statistics for each wallet
    node_stats: HashMap<WalletAddress, NodeStats>,
    /// Configuration
    config: GraphAnalyzerConfig,
    /// Metrics
    metrics: GraphMetrics,
}

impl TransactionGraph {
    /// Create a new empty transaction graph
    pub fn new(config: GraphAnalyzerConfig) -> Self {
        Self {
            graph: DiGraph::new(),
            wallet_to_node: HashMap::new(),
            node_stats: HashMap::new(),
            config,
            metrics: GraphMetrics::new(),
        }
    }

    /// Get or create a node for a wallet address
    fn get_or_create_node(&mut self, wallet: &str) -> NodeIndex {
        if let Some(&idx) = self.wallet_to_node.get(wallet) {
            return idx;
        }

        // Check max wallets limit
        if self.wallet_to_node.len() >= self.config.max_wallets {
            warn!("Max wallet limit reached: {}", self.config.max_wallets);
            // Return a dummy node or handle gracefully
            return self.wallet_to_node.values().next().copied().unwrap();
        }

        let idx = self.graph.add_node(wallet.to_string());
        self.wallet_to_node.insert(wallet.to_string(), idx);
        self.node_stats
            .insert(wallet.to_string(), NodeStats::default());
        idx
    }

    /// Add a transaction edge to the graph
    pub fn add_transaction(
        &mut self,
        from: &str,
        to: &str,
        edge_data: TransactionEdge,
    ) -> Result<()> {
        let from_idx = self.get_or_create_node(from);
        let to_idx = self.get_or_create_node(to);

        // Add edge
        self.graph.add_edge(from_idx, to_idx, edge_data.clone());

        // Update node statistics
        if let Some(stats) = self.node_stats.get_mut(from) {
            stats.out_degree += 1;
            stats.total_sent += edge_data.amount;
            stats.last_seen = Some(edge_data.timestamp);
            if stats.first_seen.is_none() {
                stats.first_seen = Some(edge_data.timestamp);
            }
        }

        if let Some(stats) = self.node_stats.get_mut(to) {
            stats.in_degree += 1;
            stats.total_received += edge_data.amount;
            stats.last_seen = Some(edge_data.timestamp);
            if stats.first_seen.is_none() {
                stats.first_seen = Some(edge_data.timestamp);
            }
        }

        self.metrics.edge_count += 1;

        Ok(())
    }

    /// Build graph from token transactions via RPC
    pub async fn build_from_token(
        &mut self,
        rpc_client: Arc<RpcClient>,
        token_mint: &str,
        limit: Option<usize>,
    ) -> Result<()> {
        let start = Instant::now();
        info!("Building transaction graph for token: {}", token_mint);

        let mint_pubkey = Pubkey::from_str(token_mint).context("Invalid token mint pubkey")?;

        // Get token accounts (simplified - in production, use get_token_accounts_by_owner)
        // For now, we'll build a synthetic graph for testing
        // In real implementation, you'd fetch actual transactions using RPC methods like:
        // - getSignaturesForAddress
        // - getTransaction for each signature
        // - Parse transaction data to extract transfers

        let limit = limit.unwrap_or(self.config.max_transactions);
        debug!("Fetching up to {} transactions", limit);

        // Placeholder: In production, implement actual RPC calls here
        // This would involve:
        // 1. rpc_client.get_signatures_for_address(&mint_pubkey, ...)
        // 2. For each signature, get_transaction and parse transfers
        // 3. Build graph edges from parsed data

        self.metrics.build_time_ms = start.elapsed().as_millis() as u64;
        self.metrics.node_count = self.wallet_to_node.len();

        info!(
            "Graph built in {}ms: {} nodes, {} edges",
            self.metrics.build_time_ms, self.metrics.node_count, self.metrics.edge_count
        );

        Ok(())
    }

    /// Get node statistics for a wallet
    pub fn get_node_stats(&self, wallet: &str) -> Option<&NodeStats> {
        self.node_stats.get(wallet)
    }

    /// Get all nodes in the graph
    pub fn nodes(&self) -> Vec<WalletAddress> {
        self.wallet_to_node.keys().cloned().collect()
    }

    /// Get edges from a specific wallet
    pub fn get_edges_from(&self, wallet: &str) -> Vec<&TransactionEdge> {
        if let Some(&node_idx) = self.wallet_to_node.get(wallet) {
            self.graph
                .edges(node_idx)
                .map(|edge| edge.weight())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get edges to a specific wallet
    pub fn get_edges_to(&self, wallet: &str) -> Vec<&TransactionEdge> {
        if let Some(&node_idx) = self.wallet_to_node.get(wallet) {
            self.graph
                .edges_directed(node_idx, petgraph::Direction::Incoming)
                .map(|edge| edge.weight())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Calculate graph metrics
    pub fn calculate_metrics(&mut self) {
        self.metrics.node_count = self.wallet_to_node.len();
        self.metrics.edge_count = self.graph.edge_count();

        // Calculate density
        let n = self.metrics.node_count as f64;
        if n > 1.0 {
            let possible_edges = n * (n - 1.0);
            self.metrics.density = self.metrics.edge_count as f64 / possible_edges;
        }

        // Count connected components using petgraph
        self.metrics.connected_components = petgraph::algo::connected_components(&self.graph);

        // Estimate memory usage
        self.metrics.memory_usage_bytes =
            std::mem::size_of::<DiGraph<WalletAddress, TransactionEdge>>()
                + self.graph.node_count() * std::mem::size_of::<WalletAddress>()
                + self.graph.edge_count() * std::mem::size_of::<TransactionEdge>()
                + self.wallet_to_node.len()
                    * (std::mem::size_of::<WalletAddress>() + std::mem::size_of::<NodeIndex>())
                + self.node_stats.len()
                    * (std::mem::size_of::<WalletAddress>() + std::mem::size_of::<NodeStats>());
    }

    /// Get current metrics
    pub fn metrics(&self) -> &GraphMetrics {
        &self.metrics
    }

    /// Clear the graph
    pub fn clear(&mut self) {
        self.graph.clear();
        self.wallet_to_node.clear();
        self.node_stats.clear();
        self.metrics = GraphMetrics::new();
    }

    /// Get the internal graph (for advanced algorithms)
    pub(crate) fn graph(&self) -> &DiGraph<WalletAddress, TransactionEdge> {
        &self.graph
    }

    /// Get wallet to node mapping
    pub(crate) fn wallet_to_node(&self) -> &HashMap<WalletAddress, NodeIndex> {
        &self.wallet_to_node
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_edge(amount: u64) -> TransactionEdge {
        TransactionEdge {
            signature: "test_sig".to_string(),
            amount,
            timestamp: Utc::now(),
            slot: 12345,
            token_mint: Some("test_mint".to_string()),
        }
    }

    #[test]
    fn test_graph_creation() {
        let config = GraphAnalyzerConfig::default();
        let graph = TransactionGraph::new(config);
        assert_eq!(graph.metrics().node_count, 0);
        assert_eq!(graph.metrics().edge_count, 0);
    }

    #[test]
    fn test_add_transaction() {
        let config = GraphAnalyzerConfig::default();
        let mut graph = TransactionGraph::new(config);

        let edge = create_test_edge(1000);
        graph.add_transaction("wallet1", "wallet2", edge).unwrap();

        assert_eq!(graph.nodes().len(), 2);
        assert_eq!(graph.metrics().edge_count, 1);
    }

    #[test]
    fn test_node_stats() {
        let config = GraphAnalyzerConfig::default();
        let mut graph = TransactionGraph::new(config);

        let edge = create_test_edge(1000);
        graph.add_transaction("wallet1", "wallet2", edge).unwrap();

        let stats = graph.get_node_stats("wallet1").unwrap();
        assert_eq!(stats.out_degree, 1);
        assert_eq!(stats.total_sent, 1000);

        let stats = graph.get_node_stats("wallet2").unwrap();
        assert_eq!(stats.in_degree, 1);
        assert_eq!(stats.total_received, 1000);
    }

    #[test]
    fn test_calculate_metrics() {
        let config = GraphAnalyzerConfig::default();
        let mut graph = TransactionGraph::new(config);

        graph
            .add_transaction("w1", "w2", create_test_edge(100))
            .unwrap();
        graph
            .add_transaction("w2", "w3", create_test_edge(200))
            .unwrap();
        graph
            .add_transaction("w3", "w1", create_test_edge(300))
            .unwrap();

        graph.calculate_metrics();

        let metrics = graph.metrics();
        assert_eq!(metrics.node_count, 3);
        assert_eq!(metrics.edge_count, 3);
        assert!(metrics.density > 0.0);
    }

    #[test]
    fn test_max_wallets_limit() {
        let mut config = GraphAnalyzerConfig::default();
        config.max_wallets = 2;
        let mut graph = TransactionGraph::new(config);

        graph
            .add_transaction("w1", "w2", create_test_edge(100))
            .unwrap();
        // This should hit the limit
        graph
            .add_transaction("w3", "w4", create_test_edge(200))
            .unwrap();

        // Should not exceed max_wallets
        assert!(graph.nodes().len() <= 2);
    }

    #[test]
    fn test_clear_graph() {
        let config = GraphAnalyzerConfig::default();
        let mut graph = TransactionGraph::new(config);

        graph
            .add_transaction("w1", "w2", create_test_edge(100))
            .unwrap();
        assert_eq!(graph.nodes().len(), 2);

        graph.clear();
        assert_eq!(graph.nodes().len(), 0);
        assert_eq!(graph.metrics().edge_count, 0);
    }
}
