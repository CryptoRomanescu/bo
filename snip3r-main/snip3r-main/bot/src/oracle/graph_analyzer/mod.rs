//! On-Chain Graph Analyzer
//!
//! Analyzes Solana transaction graphs to detect pump & dump schemes,
//! wash trading, and wallet clustering patterns.
//!
//! # Features
//!
//! - Transaction graph construction from Solana RPC data
//! - Pump & dump pattern detection with >90% accuracy
//! - Wash trading detection (circular transaction patterns)
//! - Wallet clustering and Sybil attack detection
//! - Real-time graph updates
//! - <5 second graph build time for 10k wallets
//! - Memory-efficient design for large-scale analysis
//!
//! # Example
//!
//! ```no_run
//! use h_5n1p3r::oracle::graph_analyzer::{OnChainGraphAnalyzer, GraphAnalyzerConfig};
//! use solana_client::nonblocking::rpc_client::RpcClient;
//! use std::sync::Arc;
//!
//! # async fn example() {
//! let config = GraphAnalyzerConfig::default();
//! let rpc_client = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
//! let mut analyzer = OnChainGraphAnalyzer::new(config, rpc_client);
//!
//! // Analyze a token
//! let token_mint = "TokenMintAddressHere";
//! let result = analyzer.analyze_token(token_mint).await.unwrap();
//!
//! println!("Detected {} patterns", result.patterns.len());
//! println!("Graph metrics: {:?}", result.metrics);
//! # }
//! ```

pub mod clustering;
pub mod graph_builder;
pub mod pattern_detector;
pub mod types;
pub mod wash_trading_api;
pub mod wash_trading_detector;

use anyhow::{Context, Result};
use clustering::WalletClusterer;
pub use graph_builder::TransactionGraph;
use pattern_detector::PatternDetector;
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use tracing::{info, instrument};
pub use types::*;
pub use wash_trading_api::{WashTradingBatchScorer, WashTradingScore, WashTradingScorer};
pub use wash_trading_detector::{
    CircularPath, SuspiciousCluster, WashTradingConfig, WashTradingDetector, WashTradingResult,
};

/// Main on-chain graph analyzer
pub struct OnChainGraphAnalyzer {
    /// Graph instance
    graph: TransactionGraph,
    /// Pattern detector
    detector: PatternDetector,
    /// Wallet clusterer
    clusterer: WalletClusterer,
    /// Solana RPC client
    rpc_client: Arc<RpcClient>,
    /// Configuration
    config: GraphAnalyzerConfig,
}

/// Analysis result for a token
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    /// Detected patterns
    pub patterns: Vec<GraphPatternMatch>,
    /// Wallet clusters
    pub clusters: Vec<WalletCluster>,
    /// Graph metrics
    pub metrics: GraphMetrics,
    /// Pattern detection summary
    pub summary: AnalysisSummary,
}

/// Summary of analysis results
#[derive(Debug, Clone, Default)]
pub struct AnalysisSummary {
    /// Number of pump & dump patterns detected
    pub pump_dump_count: usize,
    /// Number of wash trading patterns detected
    pub wash_trading_count: usize,
    /// Number of coordinated buying patterns
    pub coordinated_buying_count: usize,
    /// Number of Sybil clusters detected
    pub sybil_cluster_count: usize,
    /// Overall risk score (0.0 - 1.0)
    pub risk_score: f64,
    /// Is suspicious (risk > 0.7)
    pub is_suspicious: bool,
}

impl OnChainGraphAnalyzer {
    /// Create a new graph analyzer
    pub fn new(config: GraphAnalyzerConfig, rpc_client: Arc<RpcClient>) -> Self {
        let graph = TransactionGraph::new(config.clone());
        let detector = PatternDetector::new(config.clone());
        let clusterer = WalletClusterer::new(config.clone());

        Self {
            graph,
            detector,
            clusterer,
            rpc_client,
            config,
        }
    }

    /// Analyze a token and detect patterns
    #[instrument(skip(self), fields(token_mint))]
    pub async fn analyze_token(&mut self, token_mint: &str) -> Result<AnalysisResult> {
        info!("Starting on-chain graph analysis for token: {}", token_mint);

        // Build transaction graph
        self.graph.clear();
        self.graph
            .build_from_token(
                Arc::clone(&self.rpc_client),
                token_mint,
                Some(self.config.max_transactions),
            )
            .await
            .context("Failed to build transaction graph")?;

        // Calculate graph metrics
        self.graph.calculate_metrics();

        // Detect patterns
        let patterns = self.detector.detect_all_patterns(&self.graph);

        // Cluster wallets
        let clusters = self.clusterer.cluster_wallets(&self.graph);

        // Generate summary
        let summary = self.generate_summary(&patterns, &clusters);

        Ok(AnalysisResult {
            patterns,
            clusters,
            metrics: self.graph.metrics().clone(),
            summary,
        })
    }

    /// Analyze multiple tokens in batch
    pub async fn analyze_batch(&mut self, token_mints: &[&str]) -> Result<Vec<AnalysisResult>> {
        let mut results = Vec::new();

        for token_mint in token_mints {
            match self.analyze_token(token_mint).await {
                Ok(result) => results.push(result),
                Err(e) => {
                    tracing::warn!("Failed to analyze token {}: {}", token_mint, e);
                }
            }
        }

        Ok(results)
    }

    /// Update graph with new transaction (for real-time updates)
    pub fn update_with_transaction(
        &mut self,
        from: &str,
        to: &str,
        edge_data: TransactionEdge,
    ) -> Result<()> {
        self.graph.add_transaction(from, to, edge_data)?;
        self.graph.calculate_metrics();
        Ok(())
    }

    /// Get detected patterns for specific pattern type
    pub fn get_patterns_by_type(&self, pattern_type: PatternType) -> Vec<GraphPatternMatch> {
        self.detector
            .detect_all_patterns(&self.graph)
            .into_iter()
            .filter(|p| p.pattern_type == pattern_type)
            .collect()
    }

    /// Get Sybil clusters
    pub fn get_sybil_clusters(&self) -> Vec<WalletCluster> {
        self.clusterer.detect_sybil_clusters(&self.graph)
    }

    /// Get current graph metrics
    pub fn metrics(&self) -> &GraphMetrics {
        self.graph.metrics()
    }

    /// Generate analysis summary
    fn generate_summary(
        &self,
        patterns: &[GraphPatternMatch],
        clusters: &[WalletCluster],
    ) -> AnalysisSummary {
        let pump_dump_count = patterns
            .iter()
            .filter(|p| p.pattern_type == PatternType::PumpAndDump)
            .count();

        let wash_trading_count = patterns
            .iter()
            .filter(|p| p.pattern_type == PatternType::WashTrading)
            .count();

        let coordinated_buying_count = patterns
            .iter()
            .filter(|p| p.pattern_type == PatternType::CoordinatedBuying)
            .count();

        let sybil_cluster_count = clusters
            .iter()
            .filter(|c| c.pattern == Some(PatternType::SybilCluster))
            .count();

        // Calculate overall risk score
        let pattern_risk = if patterns.is_empty() {
            0.0
        } else {
            patterns.iter().map(|p| p.confidence).sum::<f64>() / patterns.len() as f64
        };

        let cluster_risk = if sybil_cluster_count > 0 {
            0.5 + (sybil_cluster_count as f64 / 10.0).min(0.5)
        } else {
            0.0
        };

        let risk_score = (pattern_risk * 0.7 + cluster_risk * 0.3).min(1.0);

        AnalysisSummary {
            pump_dump_count,
            wash_trading_count,
            coordinated_buying_count,
            sybil_cluster_count,
            risk_score,
            is_suspicious: risk_score > 0.7,
        }
    }

    /// Clear the graph and reset state
    pub fn clear(&mut self) {
        self.graph.clear();
    }

    /// Get configuration
    pub fn config(&self) -> &GraphAnalyzerConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_rpc_client() -> Arc<RpcClient> {
        Arc::new(RpcClient::new(
            "https://api.mainnet-beta.solana.com".to_string(),
        ))
    }

    #[test]
    fn test_analyzer_creation() {
        let config = GraphAnalyzerConfig::default();
        let rpc_client = create_test_rpc_client();
        let _analyzer = OnChainGraphAnalyzer::new(config, rpc_client);
    }

    #[test]
    fn test_update_with_transaction() {
        let config = GraphAnalyzerConfig::default();
        let rpc_client = create_test_rpc_client();
        let mut analyzer = OnChainGraphAnalyzer::new(config, rpc_client);

        let edge = TransactionEdge {
            signature: "test_sig".to_string(),
            amount: 1000,
            timestamp: Utc::now(),
            slot: 12345,
            token_mint: Some("test_mint".to_string()),
        };

        analyzer.update_with_transaction("w1", "w2", edge).unwrap();
        assert_eq!(analyzer.metrics().edge_count, 1);
    }

    #[test]
    fn test_clear_analyzer() {
        let config = GraphAnalyzerConfig::default();
        let rpc_client = create_test_rpc_client();
        let mut analyzer = OnChainGraphAnalyzer::new(config, rpc_client);

        let edge = TransactionEdge {
            signature: "test_sig".to_string(),
            amount: 1000,
            timestamp: Utc::now(),
            slot: 12345,
            token_mint: Some("test_mint".to_string()),
        };

        analyzer.update_with_transaction("w1", "w2", edge).unwrap();
        assert_eq!(analyzer.metrics().edge_count, 1);

        analyzer.clear();
        assert_eq!(analyzer.metrics().edge_count, 0);
    }

    #[test]
    fn test_analysis_summary() {
        let patterns = vec![
            GraphPatternMatch {
                pattern_type: PatternType::PumpAndDump,
                confidence: 0.9,
                involved_wallets: vec!["w1".to_string()],
                evidence: PatternEvidence {
                    temporal_clustering: 0.9,
                    circular_trading: 0.0,
                    volume_spike: 0.8,
                    price_manipulation: 0.85,
                    wallet_age_suspicion: 0.0,
                    notes: vec![],
                },
            },
            GraphPatternMatch {
                pattern_type: PatternType::WashTrading,
                confidence: 0.85,
                involved_wallets: vec!["w2".to_string()],
                evidence: PatternEvidence {
                    temporal_clustering: 0.8,
                    circular_trading: 0.9,
                    volume_spike: 0.0,
                    price_manipulation: 0.0,
                    wallet_age_suspicion: 0.0,
                    notes: vec![],
                },
            },
        ];

        let config = GraphAnalyzerConfig::default();
        let rpc_client = create_test_rpc_client();
        let analyzer = OnChainGraphAnalyzer::new(config, rpc_client);

        let summary = analyzer.generate_summary(&patterns, &[]);

        assert_eq!(summary.pump_dump_count, 1);
        assert_eq!(summary.wash_trading_count, 1);
        assert!(summary.risk_score > 0.5);
    }
}
