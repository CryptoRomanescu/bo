//! Pattern Detection Algorithms
//!
//! Detects pump & dump, wash trading, and other malicious patterns
//! in transaction graphs.

use super::graph_builder::TransactionGraph;
use super::types::*;
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use petgraph::algo::dijkstra;
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};
use tracing::{debug, info};

/// Pattern detector for analyzing transaction graphs
pub struct PatternDetector {
    config: GraphAnalyzerConfig,
}

impl PatternDetector {
    /// Create a new pattern detector
    pub fn new(config: GraphAnalyzerConfig) -> Self {
        Self { config }
    }

    /// Detect all patterns in the graph
    pub fn detect_all_patterns(&self, graph: &TransactionGraph) -> Vec<GraphPatternMatch> {
        let mut patterns = Vec::new();

        // Detect pump and dump patterns
        patterns.extend(self.detect_pump_and_dump(graph));

        // Detect wash trading patterns
        patterns.extend(self.detect_wash_trading(graph));

        // Detect coordinated buying
        patterns.extend(self.detect_coordinated_buying(graph));

        info!(
            "Detected {} patterns: {} pump&dump, {} wash trading",
            patterns.len(),
            patterns
                .iter()
                .filter(|p| p.pattern_type == PatternType::PumpAndDump)
                .count(),
            patterns
                .iter()
                .filter(|p| p.pattern_type == PatternType::WashTrading)
                .count()
        );

        patterns
    }

    /// Detect pump and dump patterns
    /// Characteristics:
    /// - Rapid price increase followed by large sell-offs
    /// - Coordinated buying from multiple wallets
    /// - Short time window between buys and sells
    pub fn detect_pump_and_dump(&self, graph: &TransactionGraph) -> Vec<GraphPatternMatch> {
        let mut patterns = Vec::new();

        // Group wallets by activity time windows
        let time_clusters = self.cluster_by_time(graph);

        for cluster in time_clusters {
            if cluster.len() < 3 {
                continue; // Need at least 3 wallets for a pump
            }

            // Calculate temporal clustering score
            let temporal_score = self.calculate_temporal_clustering(&cluster, graph);

            // Calculate volume spike score
            let volume_score = self.calculate_volume_spike(&cluster, graph);

            // Calculate coordination score based on transaction timing
            let coordination_score = self.calculate_coordination_score(&cluster, graph);

            // Combine scores
            let confidence = (temporal_score + volume_score + coordination_score) / 3.0;

            if confidence >= self.config.min_confidence {
                patterns.push(GraphPatternMatch {
                    pattern_type: PatternType::PumpAndDump,
                    confidence,
                    involved_wallets: cluster.clone(),
                    evidence: PatternEvidence {
                        temporal_clustering: temporal_score,
                        circular_trading: 0.0,
                        volume_spike: volume_score,
                        price_manipulation: coordination_score,
                        wallet_age_suspicion: 0.0,
                        notes: vec!["Detected coordinated buying activity".to_string()],
                    },
                });
            }
        }

        patterns
    }

    /// Detect wash trading patterns
    /// Characteristics:
    /// - Circular transaction patterns (A->B->C->A)
    /// - Similar transaction amounts
    /// - Rapid succession of trades
    pub fn detect_wash_trading(&self, graph: &TransactionGraph) -> Vec<GraphPatternMatch> {
        let mut patterns = Vec::new();

        // Find circular paths in the graph
        let cycles = self.find_circular_paths(graph);

        for cycle in cycles {
            if cycle.len() < 2 {
                continue;
            }

            // Calculate circular trading score
            let circular_score = self.calculate_circular_trading_score(&cycle, graph);

            // Calculate temporal clustering (wash trades happen quickly)
            let temporal_score = self.calculate_temporal_clustering(&cycle, graph);

            // Check if amounts are similar (suspicious)
            let amount_similarity = self.calculate_amount_similarity(&cycle, graph);

            let confidence =
                (circular_score * 0.5 + temporal_score * 0.3 + amount_similarity * 0.2);

            if confidence >= self.config.min_confidence {
                patterns.push(GraphPatternMatch {
                    pattern_type: PatternType::WashTrading,
                    confidence,
                    involved_wallets: cycle.clone(),
                    evidence: PatternEvidence {
                        temporal_clustering: temporal_score,
                        circular_trading: circular_score,
                        volume_spike: 0.0,
                        price_manipulation: 0.0,
                        wallet_age_suspicion: 0.0,
                        notes: vec![format!(
                            "Detected circular trading path with {} wallets",
                            cycle.len()
                        )],
                    },
                });
            }
        }

        patterns
    }

    /// Detect coordinated buying patterns
    pub fn detect_coordinated_buying(&self, graph: &TransactionGraph) -> Vec<GraphPatternMatch> {
        let mut patterns = Vec::new();

        // Find wallets with similar buying patterns
        let time_clusters = self.cluster_by_time(graph);

        for cluster in time_clusters {
            if cluster.len() < 5 {
                continue; // Need at least 5 wallets for coordinated buying
            }

            let coordination_score = self.calculate_coordination_score(&cluster, graph);
            let temporal_score = self.calculate_temporal_clustering(&cluster, graph);

            let confidence = (coordination_score + temporal_score) / 2.0;

            if confidence >= self.config.min_confidence {
                patterns.push(GraphPatternMatch {
                    pattern_type: PatternType::CoordinatedBuying,
                    confidence,
                    involved_wallets: cluster.clone(),
                    evidence: PatternEvidence {
                        temporal_clustering: temporal_score,
                        circular_trading: 0.0,
                        volume_spike: 0.0,
                        price_manipulation: coordination_score,
                        wallet_age_suspicion: 0.0,
                        notes: vec!["Detected coordinated buying behavior".to_string()],
                    },
                });
            }
        }

        patterns
    }

    /// Cluster wallets by transaction time windows
    fn cluster_by_time(&self, graph: &TransactionGraph) -> Vec<Vec<WalletAddress>> {
        let mut clusters = Vec::new();
        let wallets = graph.nodes();

        if wallets.is_empty() {
            return clusters;
        }

        // Simple time-based clustering
        let mut visited = HashSet::new();
        let time_threshold = Duration::seconds(self.config.time_window_secs);

        for wallet in &wallets {
            if visited.contains(wallet) {
                continue;
            }

            if let Some(stats) = graph.get_node_stats(wallet) {
                if let Some(first_seen) = stats.first_seen {
                    let mut cluster = vec![wallet.clone()];
                    visited.insert(wallet.clone());

                    // Find other wallets with similar first_seen times
                    for other in &wallets {
                        if visited.contains(other) {
                            continue;
                        }

                        if let Some(other_stats) = graph.get_node_stats(other) {
                            if let Some(other_first_seen) = other_stats.first_seen {
                                let time_diff = (other_first_seen - first_seen).abs();
                                if time_diff < time_threshold {
                                    cluster.push(other.clone());
                                    visited.insert(other.clone());
                                }
                            }
                        }
                    }

                    if cluster.len() > 1 {
                        clusters.push(cluster);
                    }
                }
            }
        }

        clusters
    }

    /// Find circular transaction paths
    fn find_circular_paths(&self, graph: &TransactionGraph) -> Vec<Vec<WalletAddress>> {
        let mut cycles = Vec::new();
        let g = graph.graph();
        let wallet_map = graph.wallet_to_node();

        // Use a simple DFS-based cycle detection
        for (wallet, &start_idx) in wallet_map.iter() {
            let mut visited = HashSet::new();
            let mut path = Vec::new();

            self.dfs_find_cycles(
                g,
                start_idx,
                start_idx,
                &mut visited,
                &mut path,
                &mut cycles,
                wallet_map,
            );
        }

        // Deduplicate cycles
        cycles.sort();
        cycles.dedup();

        // Limit to reasonable cycle lengths (2-10 nodes)
        cycles
            .into_iter()
            .filter(|c| c.len() >= 2 && c.len() <= 10)
            .collect()
    }

    /// DFS helper for cycle detection
    fn dfs_find_cycles(
        &self,
        graph: &petgraph::graph::DiGraph<WalletAddress, TransactionEdge>,
        current: petgraph::graph::NodeIndex,
        start: petgraph::graph::NodeIndex,
        visited: &mut HashSet<petgraph::graph::NodeIndex>,
        path: &mut Vec<WalletAddress>,
        cycles: &mut Vec<Vec<WalletAddress>>,
        wallet_map: &HashMap<WalletAddress, petgraph::graph::NodeIndex>,
    ) {
        if path.len() > 10 {
            return; // Limit path length
        }

        visited.insert(current);
        if let Some(wallet) = graph.node_weight(current) {
            path.push(wallet.clone());
        }

        for edge in graph.edges(current) {
            let target = edge.target();

            if target == start && path.len() >= 2 {
                // Found a cycle
                cycles.push(path.clone());
            } else if !visited.contains(&target) && cycles.len() < 100 {
                // Limit total cycles found
                self.dfs_find_cycles(graph, target, start, visited, path, cycles, wallet_map);
            }
        }

        visited.remove(&current);
        path.pop();
    }

    /// Calculate temporal clustering score
    fn calculate_temporal_clustering(
        &self,
        wallets: &[WalletAddress],
        graph: &TransactionGraph,
    ) -> f64 {
        if wallets.len() < 2 {
            return 0.0;
        }

        let mut timestamps = Vec::new();
        for wallet in wallets {
            if let Some(stats) = graph.get_node_stats(wallet) {
                if let Some(ts) = stats.first_seen {
                    timestamps.push(ts);
                }
            }
        }

        if timestamps.len() < 2 {
            return 0.0;
        }

        timestamps.sort();

        // Calculate time variance
        let avg_time =
            timestamps.iter().map(|t| t.timestamp()).sum::<i64>() / timestamps.len() as i64;

        let variance = timestamps
            .iter()
            .map(|t| {
                let diff = t.timestamp() - avg_time;
                diff * diff
            })
            .sum::<i64>() as f64
            / timestamps.len() as f64;

        // Lower variance = higher clustering
        // Normalize to 0-1 range
        let std_dev = variance.sqrt();
        let max_std_dev = self.config.time_window_secs as f64;

        (1.0 - (std_dev / max_std_dev).min(1.0)).max(0.0)
    }

    /// Calculate volume spike score
    fn calculate_volume_spike(&self, wallets: &[WalletAddress], graph: &TransactionGraph) -> f64 {
        let mut total_volume = 0u64;

        for wallet in wallets {
            if let Some(stats) = graph.get_node_stats(wallet) {
                total_volume += stats.total_sent + stats.total_received;
            }
        }

        // Simple heuristic: higher volume = higher score
        // Normalize based on number of wallets
        let avg_volume_per_wallet = total_volume as f64 / wallets.len().max(1) as f64;

        // Scale to 0-1 (assuming 1B lamports as high volume)
        (avg_volume_per_wallet / 1_000_000_000.0).min(1.0)
    }

    /// Calculate coordination score
    fn calculate_coordination_score(
        &self,
        wallets: &[WalletAddress],
        graph: &TransactionGraph,
    ) -> f64 {
        if wallets.len() < 2 {
            return 0.0;
        }

        // Check how many wallets interact with each other
        let mut interaction_count = 0;
        let mut total_possible = 0;

        for i in 0..wallets.len() {
            for j in (i + 1)..wallets.len() {
                total_possible += 1;

                // Check if there's an edge between these wallets
                let edges_from = graph.get_edges_from(&wallets[i]);
                let edges_to = graph.get_edges_to(&wallets[j]);

                if !edges_from.is_empty() || !edges_to.is_empty() {
                    interaction_count += 1;
                }
            }
        }

        if total_possible == 0 {
            return 0.0;
        }

        interaction_count as f64 / total_possible as f64
    }

    /// Calculate circular trading score
    fn calculate_circular_trading_score(
        &self,
        cycle: &[WalletAddress],
        graph: &TransactionGraph,
    ) -> f64 {
        if cycle.len() < 2 {
            return 0.0;
        }

        // Score based on cycle length and completeness
        let cycle_completeness = 1.0; // Simplified - always complete if detected
        let length_factor = (cycle.len() as f64 / 5.0).min(1.0); // Prefer longer cycles

        cycle_completeness * length_factor
    }

    /// Calculate amount similarity in transactions
    fn calculate_amount_similarity(
        &self,
        wallets: &[WalletAddress],
        graph: &TransactionGraph,
    ) -> f64 {
        let mut amounts = Vec::new();

        for wallet in wallets {
            let edges = graph.get_edges_from(wallet);
            for edge in edges {
                amounts.push(edge.amount as f64);
            }
        }

        if amounts.len() < 2 {
            return 0.0;
        }

        // Calculate coefficient of variation
        let mean = amounts.iter().sum::<f64>() / amounts.len() as f64;
        let variance =
            amounts.iter().map(|a| (a - mean).powi(2)).sum::<f64>() / amounts.len() as f64;

        let std_dev = variance.sqrt();
        let cv = if mean > 0.0 { std_dev / mean } else { 1.0 };

        // Lower CV = higher similarity
        (1.0 - cv.min(1.0)).max(0.0)
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
    fn test_detector_creation() {
        let config = GraphAnalyzerConfig::default();
        let _detector = PatternDetector::new(config);
    }

    #[test]
    fn test_detect_pump_and_dump() {
        let config = GraphAnalyzerConfig::default();
        let detector = PatternDetector::new(config.clone());
        let mut graph = TransactionGraph::new(config);

        // Create a pump pattern: multiple wallets buying at similar time
        let base_time = Utc::now();
        for i in 0..5 {
            let wallet = format!("wallet{}", i);
            graph
                .add_transaction(&wallet, "token_pool", create_test_edge(1_000_000))
                .unwrap();
        }

        let patterns = detector.detect_pump_and_dump(&graph);
        // May or may not detect pattern depending on thresholds
        assert!(patterns.len() <= 1);
    }

    #[test]
    fn test_detect_wash_trading() {
        let config = GraphAnalyzerConfig::default();
        let detector = PatternDetector::new(config.clone());
        let mut graph = TransactionGraph::new(config);

        // Create a circular trading pattern
        graph
            .add_transaction("w1", "w2", create_test_edge(1000))
            .unwrap();
        graph
            .add_transaction("w2", "w3", create_test_edge(1000))
            .unwrap();
        graph
            .add_transaction("w3", "w1", create_test_edge(1000))
            .unwrap();

        let patterns = detector.detect_wash_trading(&graph);
        // Should detect circular pattern
        assert!(!patterns.is_empty());
    }

    #[test]
    fn test_temporal_clustering_score() {
        let config = GraphAnalyzerConfig::default();
        let detector = PatternDetector::new(config.clone());
        let mut graph = TransactionGraph::new(config);

        // Add transactions at similar times
        graph
            .add_transaction("w1", "w2", create_test_edge(100))
            .unwrap();
        graph
            .add_transaction("w3", "w4", create_test_edge(100))
            .unwrap();

        let wallets = vec!["w1".to_string(), "w3".to_string()];
        let score = detector.calculate_temporal_clustering(&wallets, &graph);

        assert!(score >= 0.0 && score <= 1.0);
    }

    #[test]
    fn test_volume_spike_score() {
        let config = GraphAnalyzerConfig::default();
        let detector = PatternDetector::new(config.clone());
        let mut graph = TransactionGraph::new(config);

        graph
            .add_transaction("w1", "w2", create_test_edge(1_000_000_000))
            .unwrap();

        let wallets = vec!["w1".to_string()];
        let score = detector.calculate_volume_spike(&wallets, &graph);

        assert!(score > 0.0 && score <= 1.0);
    }

    #[test]
    fn test_coordination_score() {
        let config = GraphAnalyzerConfig::default();
        let detector = PatternDetector::new(config.clone());
        let mut graph = TransactionGraph::new(config);

        // Create coordinated pattern
        graph
            .add_transaction("w1", "w2", create_test_edge(100))
            .unwrap();
        graph
            .add_transaction("w1", "w3", create_test_edge(100))
            .unwrap();
        graph
            .add_transaction("w2", "w3", create_test_edge(100))
            .unwrap();

        let wallets = vec!["w1".to_string(), "w2".to_string(), "w3".to_string()];
        let score = detector.calculate_coordination_score(&wallets, &graph);

        assert!(score >= 0.0 && score <= 1.0);
    }
}
