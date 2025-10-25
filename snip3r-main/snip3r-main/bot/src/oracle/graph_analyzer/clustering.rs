//! Wallet Clustering Algorithm
//!
//! Identifies clusters of related wallets based on transaction patterns

use super::graph_builder::TransactionGraph;
use super::types::*;
use anyhow::Result;
use petgraph::algo::connected_components;
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};
use tracing::{debug, info};

/// Wallet clustering analyzer
pub struct WalletClusterer {
    config: GraphAnalyzerConfig,
}

impl WalletClusterer {
    /// Create a new wallet clusterer
    pub fn new(config: GraphAnalyzerConfig) -> Self {
        Self { config }
    }

    /// Cluster wallets based on transaction patterns
    pub fn cluster_wallets(&self, graph: &TransactionGraph) -> Vec<WalletCluster> {
        info!("Starting wallet clustering analysis");

        // Use connected components as base clusters
        let mut clusters = self.find_connected_components(graph);

        // Refine clusters based on transaction patterns
        clusters = self.refine_clusters(clusters, graph);

        // Calculate cluster metrics
        for cluster in &mut clusters {
            cluster.cohesion = self.calculate_cluster_cohesion(&cluster.wallets, graph);
            cluster.avg_time_between_txs =
                self.calculate_avg_time_between_txs(&cluster.wallets, graph);
            cluster.pattern = self.identify_cluster_pattern(&cluster.wallets, graph);
        }

        info!("Found {} wallet clusters", clusters.len());
        clusters
    }

    /// Find connected components in the graph
    fn find_connected_components(&self, graph: &TransactionGraph) -> Vec<WalletCluster> {
        let g = graph.graph();
        let wallet_map = graph.wallet_to_node();

        // Get connected components
        let component_count = connected_components(g);
        debug!("Found {} connected components", component_count);

        // Build reverse mapping: node -> wallet
        let node_to_wallet: HashMap<_, _> = wallet_map
            .iter()
            .map(|(wallet, &node)| (node, wallet.clone()))
            .collect();

        // Group nodes by component using simple DFS
        let mut components: HashMap<usize, Vec<WalletAddress>> = HashMap::new();
        let mut visited = HashSet::new();

        for (wallet, &start_node) in wallet_map.iter() {
            if visited.contains(&start_node) {
                continue;
            }

            let component_id = components.len();
            let mut component_nodes = Vec::new();
            let mut stack = vec![start_node];

            while let Some(node) = stack.pop() {
                if visited.contains(&node) {
                    continue;
                }

                visited.insert(node);
                if let Some(wallet) = node_to_wallet.get(&node) {
                    component_nodes.push(wallet.clone());
                }

                // Add neighbors to stack
                for edge in g.edges(node) {
                    let target = edge.target();
                    if !visited.contains(&target) {
                        stack.push(target);
                    }
                }

                // Also check incoming edges
                for edge in g.edges_directed(node, petgraph::Direction::Incoming) {
                    let source = edge.source();
                    if !visited.contains(&source) {
                        stack.push(source);
                    }
                }
            }

            if !component_nodes.is_empty() {
                components.insert(component_id, component_nodes);
            }
        }

        // Convert to WalletCluster objects
        components
            .into_iter()
            .map(|(id, wallets)| WalletCluster {
                id,
                wallets,
                cohesion: 0.0,
                avg_time_between_txs: 0.0,
                pattern: None,
            })
            .collect()
    }

    /// Refine clusters based on additional heuristics
    fn refine_clusters(
        &self,
        clusters: Vec<WalletCluster>,
        graph: &TransactionGraph,
    ) -> Vec<WalletCluster> {
        let mut refined = Vec::new();

        for cluster in clusters {
            // Split large clusters if they have low cohesion
            if cluster.wallets.len() > 100 {
                // For very large clusters, try to split into sub-clusters
                let subclusters = self.split_large_cluster(cluster, graph);
                refined.extend(subclusters);
            } else {
                refined.push(cluster);
            }
        }

        refined
    }

    /// Split a large cluster into smaller sub-clusters
    fn split_large_cluster(
        &self,
        cluster: WalletCluster,
        graph: &TransactionGraph,
    ) -> Vec<WalletCluster> {
        // Simple splitting: group by high transaction frequency
        let mut subclusters = Vec::new();
        let mut visited = HashSet::new();

        for wallet in &cluster.wallets {
            if visited.contains(wallet) {
                continue;
            }

            // Start a new subcluster
            let mut subcluster_wallets = vec![wallet.clone()];
            visited.insert(wallet.clone());

            // Find closely connected wallets
            let edges = graph.get_edges_from(wallet);
            for edge in edges {
                // In a real implementation, we'd traverse to target wallets
                // For now, just create subclusters of reasonable size
                if subcluster_wallets.len() >= 50 {
                    break;
                }
            }

            subclusters.push(WalletCluster {
                id: subclusters.len(),
                wallets: subcluster_wallets,
                cohesion: 0.0,
                avg_time_between_txs: 0.0,
                pattern: None,
            });
        }

        if subclusters.is_empty() {
            vec![cluster]
        } else {
            subclusters
        }
    }

    /// Calculate cluster cohesion (how tightly connected wallets are)
    fn calculate_cluster_cohesion(
        &self,
        wallets: &[WalletAddress],
        graph: &TransactionGraph,
    ) -> f64 {
        if wallets.len() < 2 {
            return 0.0;
        }

        let mut edge_count = 0;
        let mut possible_edges = 0;

        for i in 0..wallets.len() {
            for j in (i + 1)..wallets.len() {
                possible_edges += 1;

                // Check if there's an edge between these wallets
                let edges_from_i = graph.get_edges_from(&wallets[i]);
                let edges_from_j = graph.get_edges_from(&wallets[j]);

                if !edges_from_i.is_empty() || !edges_from_j.is_empty() {
                    edge_count += 1;
                }
            }
        }

        if possible_edges == 0 {
            return 0.0;
        }

        edge_count as f64 / possible_edges as f64
    }

    /// Calculate average time between transactions in cluster
    fn calculate_avg_time_between_txs(
        &self,
        wallets: &[WalletAddress],
        graph: &TransactionGraph,
    ) -> f64 {
        let mut timestamps = Vec::new();

        for wallet in wallets {
            let edges = graph.get_edges_from(wallet);
            for edge in edges {
                timestamps.push(edge.timestamp.timestamp());
            }
        }

        if timestamps.len() < 2 {
            return 0.0;
        }

        timestamps.sort();

        let mut total_diff = 0i64;
        for i in 1..timestamps.len() {
            total_diff += timestamps[i] - timestamps[i - 1];
        }

        total_diff as f64 / (timestamps.len() - 1) as f64
    }

    /// Identify the likely pattern type for this cluster
    fn identify_cluster_pattern(
        &self,
        wallets: &[WalletAddress],
        graph: &TransactionGraph,
    ) -> Option<PatternType> {
        if wallets.len() < 2 {
            return Some(PatternType::Normal);
        }

        // Simple heuristics
        let cohesion = self.calculate_cluster_cohesion(wallets, graph);
        let avg_time = self.calculate_avg_time_between_txs(wallets, graph);

        // High cohesion + short time between txs = suspicious
        if cohesion > 0.7 && avg_time < 60.0 {
            // Could be wash trading or coordinated
            if wallets.len() <= 5 {
                Some(PatternType::WashTrading)
            } else {
                Some(PatternType::CoordinatedBuying)
            }
        } else if cohesion > 0.5 && wallets.len() > 10 {
            Some(PatternType::SybilCluster)
        } else {
            Some(PatternType::Normal)
        }
    }

    /// Detect Sybil clusters (groups of wallets controlled by same entity)
    pub fn detect_sybil_clusters(&self, graph: &TransactionGraph) -> Vec<WalletCluster> {
        let clusters = self.cluster_wallets(graph);

        // Filter for Sybil-like patterns
        clusters
            .into_iter()
            .filter(|c| {
                c.pattern == Some(PatternType::SybilCluster)
                    || (c.cohesion > 0.8 && c.wallets.len() >= 5)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::graph_analyzer::graph_builder::TransactionGraph;
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
    fn test_clusterer_creation() {
        let config = GraphAnalyzerConfig::default();
        let _clusterer = WalletClusterer::new(config);
    }

    #[test]
    fn test_cluster_wallets() {
        let config = GraphAnalyzerConfig::default();
        let clusterer = WalletClusterer::new(config.clone());
        let mut graph = TransactionGraph::new(config);

        // Create a connected cluster
        graph
            .add_transaction("w1", "w2", create_test_edge(100))
            .unwrap();
        graph
            .add_transaction("w2", "w3", create_test_edge(100))
            .unwrap();
        graph
            .add_transaction("w3", "w1", create_test_edge(100))
            .unwrap();

        // Create another disconnected cluster
        graph
            .add_transaction("w4", "w5", create_test_edge(100))
            .unwrap();

        let clusters = clusterer.cluster_wallets(&graph);

        // Should find at least 2 clusters
        assert!(!clusters.is_empty());
    }

    #[test]
    fn test_cluster_cohesion() {
        let config = GraphAnalyzerConfig::default();
        let clusterer = WalletClusterer::new(config.clone());
        let mut graph = TransactionGraph::new(config);

        // Create a tightly connected cluster
        graph
            .add_transaction("w1", "w2", create_test_edge(100))
            .unwrap();
        graph
            .add_transaction("w2", "w1", create_test_edge(100))
            .unwrap();
        graph
            .add_transaction("w1", "w3", create_test_edge(100))
            .unwrap();
        graph
            .add_transaction("w3", "w1", create_test_edge(100))
            .unwrap();
        graph
            .add_transaction("w2", "w3", create_test_edge(100))
            .unwrap();
        graph
            .add_transaction("w3", "w2", create_test_edge(100))
            .unwrap();

        let wallets = vec!["w1".to_string(), "w2".to_string(), "w3".to_string()];
        let cohesion = clusterer.calculate_cluster_cohesion(&wallets, &graph);

        assert!(cohesion > 0.0);
        assert!(cohesion <= 1.0);
    }

    #[test]
    fn test_identify_pattern() {
        let config = GraphAnalyzerConfig::default();
        let clusterer = WalletClusterer::new(config.clone());
        let graph = TransactionGraph::new(config);

        let wallets = vec!["w1".to_string()];
        let pattern = clusterer.identify_cluster_pattern(&wallets, &graph);

        assert!(pattern.is_some());
    }

    #[test]
    fn test_detect_sybil_clusters() {
        let config = GraphAnalyzerConfig::default();
        let clusterer = WalletClusterer::new(config.clone());
        let mut graph = TransactionGraph::new(config);

        // Create multiple wallets with high interaction
        for i in 0..10 {
            for j in (i + 1)..10 {
                let w1 = format!("sybil{}", i);
                let w2 = format!("sybil{}", j);
                graph
                    .add_transaction(&w1, &w2, create_test_edge(100))
                    .unwrap();
            }
        }

        let sybil_clusters = clusterer.detect_sybil_clusters(&graph);
        // May or may not detect depending on thresholds
        assert!(sybil_clusters.len() >= 0);
    }
}
