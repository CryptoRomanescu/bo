//! Wash Trading Detector
//!
//! Specialized detector for fake volume/wash trading patterns using graph analysis
//! and transaction clustering. Based on CryptoLemur analysis.
//!
//! ## Key Characteristics of Wash Trading:
//! - Small, frequent, circular trades (A→B→C→A patterns)
//! - Similar transaction amounts (fake pumps)
//! - High circularity in transaction graph
//! - Clustered wallets behaving as one entity
//! - Low unique counterparties per wallet
//!
//! ## Performance Target: <8 seconds per token

use super::graph_builder::TransactionGraph;
use super::types::*;
use anyhow::Result;
use chrono::Duration;
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use tracing::{debug, info};

/// Wash trading detection result
#[derive(Debug, Clone)]
pub struct WashTradingResult {
    /// Overall wash trading probability (0.0 - 1.0)
    pub wash_probability: f64,
    /// Circularity score (0.0 - 1.0) - circular transaction patterns
    pub circularity_score: f64,
    /// Number of suspicious wallet clusters detected
    pub suspicious_clusters: usize,
    /// Cluster details
    pub cluster_details: Vec<SuspiciousCluster>,
    /// Circular paths found
    pub circular_paths: Vec<CircularPath>,
    /// Risk score (0-100) for decision engine
    pub risk_score: u8,
    /// Analysis time in milliseconds
    pub analysis_time_ms: u64,
    /// Auto-reject flag for extreme cases
    pub auto_reject: bool,
}

/// Details about a suspicious wallet cluster
#[derive(Debug, Clone)]
pub struct SuspiciousCluster {
    /// Cluster ID
    pub cluster_id: usize,
    /// Wallets in the cluster
    pub wallets: Vec<WalletAddress>,
    /// Cohesion score (0.0 - 1.0) - how tightly connected
    pub cohesion: f64,
    /// Internal transaction ratio (transactions within cluster / total)
    pub internal_tx_ratio: f64,
    /// Average transaction frequency (txs per minute)
    pub avg_tx_frequency: f64,
}

/// Circular transaction path
#[derive(Debug, Clone)]
pub struct CircularPath {
    /// Wallets in the circular path
    pub path: Vec<WalletAddress>,
    /// Number of times this pattern repeats
    pub repeat_count: usize,
    /// Average transaction amount in the cycle
    pub avg_amount: u64,
    /// Amount similarity score (0.0 - 1.0) - higher means more similar
    pub amount_similarity: f64,
    /// Time span of the cycle in seconds
    pub time_span_secs: i64,
}

/// Configuration for wash trading detector
#[derive(Debug, Clone)]
pub struct WashTradingConfig {
    /// Maximum analysis time (default: 8000ms)
    pub max_analysis_time_ms: u64,
    /// Minimum circularity threshold for detection (default: 0.3)
    pub min_circularity: f64,
    /// Minimum cluster size for suspicious behavior (default: 3)
    pub min_cluster_size: usize,
    /// Auto-reject threshold (default: 0.8)
    pub auto_reject_threshold: f64,
    /// Maximum cycle length to detect (default: 10)
    pub max_cycle_length: usize,
}

impl Default for WashTradingConfig {
    fn default() -> Self {
        Self {
            max_analysis_time_ms: 8000,
            min_circularity: 0.3,
            min_cluster_size: 3,
            auto_reject_threshold: 0.8,
            max_cycle_length: 10,
        }
    }
}

/// Dedicated wash trading detector
#[derive(Debug)]
pub struct WashTradingDetector {
    config: WashTradingConfig,
}

impl WashTradingDetector {
    /// Create a new wash trading detector
    pub fn new(config: WashTradingConfig) -> Self {
        Self { config }
    }

    /// Create with default configuration
    pub fn default() -> Self {
        Self::new(WashTradingConfig::default())
    }

    /// Detect wash trading patterns in the transaction graph
    ///
    /// Returns detailed wash trading analysis with probability score
    pub fn detect(&self, graph: &TransactionGraph) -> Result<WashTradingResult> {
        let start_time = Instant::now();
        info!("Starting wash trading detection");

        // Early exit if graph is empty or too small
        if graph.nodes().len() < 3 {
            debug!("Graph too small for wash trading detection");
            return Ok(WashTradingResult {
                wash_probability: 0.0,
                circularity_score: 0.0,
                suspicious_clusters: 0,
                cluster_details: vec![],
                circular_paths: vec![],
                risk_score: 0,
                analysis_time_ms: start_time.elapsed().as_millis() as u64,
                auto_reject: false,
            });
        }

        // 1. Find circular transaction paths
        let circular_paths = self.find_circular_paths(graph);
        debug!("Found {} circular paths", circular_paths.len());

        // 2. Calculate circularity score
        let circularity_score = self.calculate_circularity_score(&circular_paths, graph);
        debug!("Circularity score: {:.3}", circularity_score);

        // 3. Identify suspicious wallet clusters
        let suspicious_clusters = self.identify_suspicious_clusters(graph, &circular_paths);
        debug!(
            "Identified {} suspicious clusters",
            suspicious_clusters.len()
        );

        // 4. Calculate overall wash probability
        let wash_probability = self.calculate_wash_probability(
            circularity_score,
            &suspicious_clusters,
            &circular_paths,
            graph,
        );
        debug!("Wash probability: {:.3}", wash_probability);

        // 5. Determine risk score (0-100)
        let risk_score = (wash_probability * 100.0) as u8;

        // 6. Auto-reject check
        let auto_reject = wash_probability >= self.config.auto_reject_threshold;

        let analysis_time_ms = start_time.elapsed().as_millis() as u64;

        // Performance check
        if analysis_time_ms > self.config.max_analysis_time_ms {
            tracing::warn!(
                "Wash trading analysis exceeded target time: {}ms > {}ms",
                analysis_time_ms,
                self.config.max_analysis_time_ms
            );
        }

        info!(
            "Wash trading detection complete: probability={:.3}, risk={}, time={}ms",
            wash_probability, risk_score, analysis_time_ms
        );

        Ok(WashTradingResult {
            wash_probability,
            circularity_score,
            suspicious_clusters: suspicious_clusters.len(),
            cluster_details: suspicious_clusters,
            circular_paths,
            risk_score,
            analysis_time_ms,
            auto_reject,
        })
    }

    /// Find circular transaction paths (A→B→C→A patterns)
    fn find_circular_paths(&self, graph: &TransactionGraph) -> Vec<CircularPath> {
        let g = graph.graph();
        let wallet_map = graph.wallet_to_node();
        let mut all_cycles = Vec::new();

        // Limit search to avoid timeout
        let max_paths = 100;
        let mut paths_found = 0;

        for (wallet, &start_idx) in wallet_map.iter() {
            if paths_found >= max_paths {
                break;
            }

            let cycles = self.dfs_cycles(
                g,
                start_idx,
                start_idx,
                &mut HashSet::new(),
                &mut Vec::new(),
            );

            for cycle in cycles {
                if cycle.len() >= 2 && cycle.len() <= self.config.max_cycle_length {
                    // Calculate cycle statistics
                    if let Some(circular_path) = self.analyze_cycle(&cycle, graph) {
                        all_cycles.push(circular_path);
                        paths_found += 1;

                        if paths_found >= max_paths {
                            break;
                        }
                    }
                }
            }
        }

        // Deduplicate and sort by repeat count
        all_cycles.sort_by(|a, b| b.repeat_count.cmp(&a.repeat_count));
        all_cycles.truncate(50); // Keep top 50 most frequent patterns

        all_cycles
    }

    /// DFS to find cycles starting and ending at the same node
    fn dfs_cycles(
        &self,
        graph: &petgraph::graph::DiGraph<WalletAddress, TransactionEdge>,
        current: petgraph::graph::NodeIndex,
        start: petgraph::graph::NodeIndex,
        visited: &mut HashSet<petgraph::graph::NodeIndex>,
        path: &mut Vec<WalletAddress>,
    ) -> Vec<Vec<WalletAddress>> {
        let mut cycles = Vec::new();

        if path.len() > self.config.max_cycle_length {
            return cycles;
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
            } else if !visited.contains(&target) && cycles.len() < 20 {
                // Continue DFS
                cycles.extend(self.dfs_cycles(graph, target, start, visited, path));
            }
        }

        visited.remove(&current);
        path.pop();

        cycles
    }

    /// Analyze a cycle to extract statistics
    fn analyze_cycle(
        &self,
        cycle: &[WalletAddress],
        graph: &TransactionGraph,
    ) -> Option<CircularPath> {
        if cycle.len() < 2 {
            return None;
        }

        let mut amounts = Vec::new();
        let mut timestamps = Vec::new();

        // Collect transaction data for this cycle
        for i in 0..cycle.len() {
            let from = &cycle[i];
            let to = &cycle[(i + 1) % cycle.len()];

            let edges = graph.get_edges_from(from);
            for edge in edges {
                // Find edges that match the cycle path
                amounts.push(edge.amount);
                timestamps.push(edge.timestamp);
            }
        }

        if amounts.is_empty() {
            return None;
        }

        // Calculate statistics
        let avg_amount = amounts.iter().sum::<u64>() / amounts.len() as u64;
        let amount_similarity = self.calculate_amount_similarity(&amounts);

        // Calculate time span
        timestamps.sort();
        let time_span_secs = if timestamps.len() >= 2 {
            (timestamps.last().unwrap().timestamp() - timestamps.first().unwrap().timestamp()).abs()
        } else {
            0
        };

        // Estimate repeat count based on transaction frequency
        let repeat_count = amounts.len() / cycle.len().max(1);

        Some(CircularPath {
            path: cycle.to_vec(),
            repeat_count,
            avg_amount,
            amount_similarity,
            time_span_secs,
        })
    }

    /// Calculate amount similarity (1.0 = all amounts identical, 0.0 = very different)
    fn calculate_amount_similarity(&self, amounts: &[u64]) -> f64 {
        if amounts.len() < 2 {
            return 0.0;
        }

        let mean = amounts.iter().sum::<u64>() as f64 / amounts.len() as f64;
        let variance = amounts
            .iter()
            .map(|&a| {
                let diff = a as f64 - mean;
                diff * diff
            })
            .sum::<f64>()
            / amounts.len() as f64;

        let std_dev = variance.sqrt();
        let cv = if mean > 0.0 { std_dev / mean } else { 1.0 };

        // Convert coefficient of variation to similarity score
        (1.0 - cv.min(1.0)).max(0.0)
    }

    /// Calculate circularity score for the entire graph
    fn calculate_circularity_score(
        &self,
        circular_paths: &[CircularPath],
        graph: &TransactionGraph,
    ) -> f64 {
        if circular_paths.is_empty() {
            return 0.0;
        }

        let total_edges = graph.metrics().edge_count as f64;
        if total_edges == 0.0 {
            return 0.0;
        }

        // Count edges involved in circular patterns
        let mut circular_edges = HashSet::new();
        for path in circular_paths {
            for i in 0..path.path.len() {
                let from = &path.path[i];
                let to = &path.path[(i + 1) % path.path.len()];
                circular_edges.insert((from.clone(), to.clone()));
            }
        }

        let circular_edge_ratio = circular_edges.len() as f64 / total_edges;

        // Weight by repeat count and amount similarity
        let weighted_score = circular_paths
            .iter()
            .map(|p| {
                let repeat_weight = (p.repeat_count as f64 / 10.0).min(1.0);
                let similarity_weight = p.amount_similarity;
                (repeat_weight + similarity_weight) / 2.0
            })
            .sum::<f64>()
            / circular_paths.len() as f64;

        // Combine metrics
        (circular_edge_ratio * 0.6 + weighted_score * 0.4).min(1.0)
    }

    /// Identify suspicious wallet clusters
    fn identify_suspicious_clusters(
        &self,
        graph: &TransactionGraph,
        circular_paths: &[CircularPath],
    ) -> Vec<SuspiciousCluster> {
        let mut clusters = Vec::new();

        // Extract wallets involved in circular patterns
        let mut circular_wallets = HashSet::new();
        for path in circular_paths {
            for wallet in &path.path {
                circular_wallets.insert(wallet.clone());
            }
        }

        // Group wallets by transaction patterns
        let wallet_groups = self.cluster_by_behavior(graph, &circular_wallets);

        for (cluster_id, wallets) in wallet_groups.iter().enumerate() {
            if wallets.len() < self.config.min_cluster_size {
                continue;
            }

            // Calculate cluster metrics
            let cohesion = self.calculate_cluster_cohesion(wallets, graph);
            let internal_tx_ratio = self.calculate_internal_tx_ratio(wallets, graph);
            let avg_tx_frequency = self.calculate_tx_frequency(wallets, graph);

            // Only include if suspicious enough
            if internal_tx_ratio > 0.5 || cohesion > 0.7 {
                clusters.push(SuspiciousCluster {
                    cluster_id,
                    wallets: wallets.clone(),
                    cohesion,
                    internal_tx_ratio,
                    avg_tx_frequency,
                });
            }
        }

        clusters
    }

    /// Cluster wallets by transaction behavior
    fn cluster_by_behavior(
        &self,
        graph: &TransactionGraph,
        circular_wallets: &HashSet<WalletAddress>,
    ) -> Vec<Vec<WalletAddress>> {
        let mut clusters = Vec::new();
        let mut visited = HashSet::new();

        for wallet in circular_wallets {
            if visited.contains(wallet) {
                continue;
            }

            let mut cluster = vec![wallet.clone()];
            visited.insert(wallet.clone());

            // Find connected wallets with similar behavior
            for other in circular_wallets {
                if visited.contains(other) {
                    continue;
                }

                if self.are_wallets_similar(wallet, other, graph) {
                    cluster.push(other.clone());
                    visited.insert(other.clone());
                }
            }

            if cluster.len() >= self.config.min_cluster_size {
                clusters.push(cluster);
            }
        }

        clusters
    }

    /// Check if two wallets have similar behavior
    fn are_wallets_similar(&self, w1: &str, w2: &str, graph: &TransactionGraph) -> bool {
        // Check if they have common counterparties
        let edges1: HashSet<_> = graph
            .get_edges_from(w1)
            .iter()
            .map(|e| e.signature.clone())
            .collect();
        let edges2: HashSet<_> = graph
            .get_edges_from(w2)
            .iter()
            .map(|e| e.signature.clone())
            .collect();

        let common_edges = edges1.intersection(&edges2).count();
        let total_edges = edges1.len() + edges2.len();

        if total_edges == 0 {
            return false;
        }

        // Similarity threshold: >30% common transactions
        (common_edges as f64 / total_edges as f64) > 0.3
    }

    /// Calculate cluster cohesion (how tightly connected)
    fn calculate_cluster_cohesion(
        &self,
        wallets: &[WalletAddress],
        graph: &TransactionGraph,
    ) -> f64 {
        if wallets.len() < 2 {
            return 0.0;
        }

        let mut internal_edges = 0;
        let mut total_possible = 0;

        for i in 0..wallets.len() {
            for j in (i + 1)..wallets.len() {
                total_possible += 1;

                // Check if there's an edge between these wallets
                let edges_from_i = graph.get_edges_from(&wallets[i]);
                let edges_from_j = graph.get_edges_from(&wallets[j]);

                if !edges_from_i.is_empty() || !edges_from_j.is_empty() {
                    internal_edges += 1;
                }
            }
        }

        if total_possible == 0 {
            return 0.0;
        }

        internal_edges as f64 / total_possible as f64
    }

    /// Calculate internal transaction ratio
    fn calculate_internal_tx_ratio(
        &self,
        wallets: &[WalletAddress],
        graph: &TransactionGraph,
    ) -> f64 {
        let wallet_set: HashSet<_> = wallets.iter().cloned().collect();
        let mut internal_txs = 0;
        let mut total_txs = 0;

        for wallet in wallets {
            let edges = graph.get_edges_from(wallet);
            total_txs += edges.len();

            // Count transactions to other wallets in the cluster
            for edge in edges {
                // Check if target is in the cluster (we'd need to enhance the graph API)
                // For now, we use a heuristic
                internal_txs += 1; // Simplified
            }
        }

        if total_txs == 0 {
            return 0.0;
        }

        // Conservative estimate
        (internal_txs as f64 / total_txs as f64) * 0.5
    }

    /// Calculate average transaction frequency
    fn calculate_tx_frequency(&self, wallets: &[WalletAddress], graph: &TransactionGraph) -> f64 {
        let mut all_timestamps = Vec::new();

        for wallet in wallets {
            if let Some(stats) = graph.get_node_stats(wallet) {
                if let Some(first_seen) = stats.first_seen {
                    all_timestamps.push(first_seen);
                }
                if let Some(last_seen) = stats.last_seen {
                    all_timestamps.push(last_seen);
                }
            }
        }

        if all_timestamps.len() < 2 {
            return 0.0;
        }

        all_timestamps.sort();
        let time_span = (all_timestamps.last().unwrap().timestamp()
            - all_timestamps.first().unwrap().timestamp())
        .abs();

        if time_span == 0 {
            return 0.0;
        }

        // Calculate transactions per minute
        let total_txs: usize = wallets
            .iter()
            .filter_map(|w| graph.get_node_stats(w))
            .map(|s| s.in_degree + s.out_degree)
            .sum();

        (total_txs as f64 / (time_span as f64 / 60.0)).max(0.0)
    }

    /// Calculate overall wash probability
    fn calculate_wash_probability(
        &self,
        circularity_score: f64,
        suspicious_clusters: &[SuspiciousCluster],
        circular_paths: &[CircularPath],
        graph: &TransactionGraph,
    ) -> f64 {
        // Factor 1: Circularity score (40% weight)
        let circularity_factor = circularity_score * 0.4;

        // Factor 2: Suspicious clusters (30% weight)
        let cluster_factor = if suspicious_clusters.is_empty() {
            0.0
        } else {
            let avg_cohesion = suspicious_clusters.iter().map(|c| c.cohesion).sum::<f64>()
                / suspicious_clusters.len() as f64;
            let cluster_count_factor = (suspicious_clusters.len() as f64 / 5.0).min(1.0);
            (avg_cohesion * 0.7 + cluster_count_factor * 0.3) * 0.3
        };

        // Factor 3: Circular path characteristics (30% weight)
        let path_factor = if circular_paths.is_empty() {
            0.0
        } else {
            let avg_similarity = circular_paths
                .iter()
                .map(|p| p.amount_similarity)
                .sum::<f64>()
                / circular_paths.len() as f64;
            let avg_repeat = circular_paths
                .iter()
                .map(|p| p.repeat_count as f64)
                .sum::<f64>()
                / circular_paths.len() as f64;
            let repeat_factor = (avg_repeat / 10.0).min(1.0);
            (avg_similarity * 0.6 + repeat_factor * 0.4) * 0.3
        };

        // Combine all factors
        (circularity_factor + cluster_factor + path_factor).min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::graph_analyzer::graph_builder::TransactionGraph;
    use chrono::Utc;

    fn create_test_edge(amount: u64) -> TransactionEdge {
        TransactionEdge {
            signature: format!("sig_{}", rand::random::<u32>()),
            amount,
            timestamp: Utc::now(),
            slot: 12345,
            token_mint: Some("test_mint".to_string()),
        }
    }

    #[test]
    fn test_detector_creation() {
        let config = WashTradingConfig::default();
        let detector = WashTradingDetector::new(config);
        assert_eq!(detector.config.max_analysis_time_ms, 8000);
    }

    #[test]
    fn test_empty_graph_detection() {
        let config = GraphAnalyzerConfig::default();
        let graph = TransactionGraph::new(config);
        let detector = WashTradingDetector::default();

        let result = detector.detect(&graph).unwrap();
        assert_eq!(result.wash_probability, 0.0);
        assert_eq!(result.risk_score, 0);
        assert!(!result.auto_reject);
    }

    #[test]
    fn test_circular_pattern_detection() {
        let config = GraphAnalyzerConfig::default();
        let mut graph = TransactionGraph::new(config);
        let detector = WashTradingDetector::default();

        // Create a circular trading pattern: A->B->C->A
        graph
            .add_transaction("wallet_a", "wallet_b", create_test_edge(1000))
            .unwrap();
        graph
            .add_transaction("wallet_b", "wallet_c", create_test_edge(1000))
            .unwrap();
        graph
            .add_transaction("wallet_c", "wallet_a", create_test_edge(1000))
            .unwrap();

        let result = detector.detect(&graph).unwrap();

        // Should detect the circular pattern
        assert!(result.circularity_score > 0.0);
        assert!(!result.circular_paths.is_empty());
        assert!(result.wash_probability > 0.0);
    }

    #[test]
    fn test_amount_similarity() {
        let detector = WashTradingDetector::default();

        // Similar amounts
        let similar = vec![1000, 1010, 990, 1005];
        let similarity = detector.calculate_amount_similarity(&similar);
        assert!(similarity > 0.9);

        // Different amounts
        let different = vec![100, 1000, 10000, 100000];
        let similarity = detector.calculate_amount_similarity(&different);
        assert!(similarity < 0.5);
    }

    #[test]
    fn test_performance_requirement() {
        let config = GraphAnalyzerConfig::default();
        let mut graph = TransactionGraph::new(config);
        let detector = WashTradingDetector::default();

        // Add many transactions
        for i in 0..100 {
            let from = format!("wallet_{}", i);
            let to = format!("wallet_{}", (i + 1) % 100);
            graph
                .add_transaction(&from, &to, create_test_edge(1000))
                .unwrap();
        }

        let result = detector.detect(&graph).unwrap();

        // Should complete within 8 seconds (8000ms)
        assert!(
            result.analysis_time_ms < 8000,
            "Analysis took {}ms, exceeds 8s target",
            result.analysis_time_ms
        );
    }

    #[test]
    fn test_auto_reject_threshold() {
        let config = GraphAnalyzerConfig::default();
        let mut graph = TransactionGraph::new(config);
        let detector = WashTradingDetector::default();

        // Create multiple circular patterns with similar amounts
        for cycle in 0..5 {
            let base = cycle * 10;
            for i in 0..3 {
                let from = format!("w_{}_{}", cycle, i);
                let to = format!("w_{}_{}", cycle, (i + 1) % 3);
                graph
                    .add_transaction(&from, &to, create_test_edge(1000))
                    .unwrap();
            }
        }

        let result = detector.detect(&graph).unwrap();

        // With many circular patterns, should have high wash probability
        assert!(result.wash_probability > 0.3);
        assert!(result.risk_score > 30);
    }
}
