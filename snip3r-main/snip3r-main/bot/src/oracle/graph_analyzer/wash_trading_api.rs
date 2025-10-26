//! Wash Trading Detector API for Scoring System
//!
//! This module provides a high-level API for integrating wash trading detection
//! into decision engines and scoring systems.

use super::graph_builder::TransactionGraph;
use super::types::*;
use super::wash_trading_detector::{WashTradingConfig, WashTradingDetector, WashTradingResult};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

/// Quick scoring API for wash trading detection
#[derive(Debug, Clone)]
pub struct WashTradingScorer {
    detector: Arc<WashTradingDetector>,
    graph_config: GraphAnalyzerConfig,
}

/// Simplified wash trading score result for scoring systems
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WashTradingScore {
    /// Risk score (0-100, higher = more risky)
    pub risk_score: u8,
    /// Wash probability (0.0-1.0)
    pub probability: f64,
    /// Auto-reject flag
    pub auto_reject: bool,
    /// Human-readable reason
    pub reason: String,
    /// Analysis time in milliseconds
    pub analysis_time_ms: u64,
}

impl WashTradingScorer {
    /// Create a new scorer with default configuration
    pub fn new() -> Self {
        let config = WashTradingConfig {
            max_analysis_time_ms: 5000, // Fast for real-time
            min_circularity: 0.3,
            auto_reject_threshold: 0.8,
            ..Default::default()
        };

        let graph_config = GraphAnalyzerConfig {
            max_transactions: 1000,
            time_window_secs: 3600,
            max_wallets: 500,
            min_confidence: 0.7,
            ..Default::default()
        };

        Self {
            detector: Arc::new(WashTradingDetector::new(config)),
            graph_config,
        }
    }

    /// Create with custom configuration
    pub fn with_config(wash_config: WashTradingConfig, graph_config: GraphAnalyzerConfig) -> Self {
        Self {
            detector: Arc::new(WashTradingDetector::new(wash_config)),
            graph_config,
        }
    }

    /// Score a token for wash trading risk
    ///
    /// # Arguments
    /// * `mint` - Token mint address
    /// * `rpc_client` - Solana RPC client
    ///
    /// # Returns
    /// Simplified score result for decision engines
    pub async fn score_token(
        &self,
        mint: &str,
        rpc_client: Arc<RpcClient>,
    ) -> Result<WashTradingScore> {
        info!("Scoring token {} for wash trading", mint);

        // Build transaction graph with timeout
        let mut graph = TransactionGraph::new(self.graph_config.clone());

        match tokio::time::timeout(
            Duration::from_secs(5),
            graph.build_from_token(rpc_client.clone(), mint, Some(1000)),
        )
        .await
        {
            Ok(Ok(_)) => {
                graph.calculate_metrics();

                // Run detection
                let result = self.detector.detect(&graph)?;

                Ok(Self::format_score(result))
            }
            Ok(Err(e)) => {
                warn!("Failed to build graph for {}: {}", mint, e);
                Err(e).context("Graph build failed")
            }
            Err(_) => {
                warn!("Graph build timeout for {}", mint);
                anyhow::bail!("Graph build timeout")
            }
        }
    }

    /// Score a token with pre-built graph (for efficiency)
    pub fn score_graph(&self, graph: &TransactionGraph) -> Result<WashTradingScore> {
        let result = self.detector.detect(graph)?;
        Ok(Self::format_score(result))
    }

    /// Format detection result into scoring API format
    fn format_score(result: WashTradingResult) -> WashTradingScore {
        let reason = if result.auto_reject {
            format!(
                "CRITICAL: High wash trading detected (prob={:.2}%, {} circular paths, {} clusters)",
                result.wash_probability * 100.0,
                result.circular_paths.len(),
                result.suspicious_clusters
            )
        } else if result.wash_probability > 0.6 {
            format!(
                "WARNING: Moderate wash trading risk (prob={:.2}%, circularity={:.2})",
                result.wash_probability * 100.0,
                result.circularity_score
            )
        } else if result.wash_probability > 0.3 {
            format!(
                "CAUTION: Some circular patterns detected (prob={:.2}%)",
                result.wash_probability * 100.0
            )
        } else {
            format!(
                "OK: Low wash trading risk (prob={:.2}%)",
                result.wash_probability * 100.0
            )
        };

        WashTradingScore {
            risk_score: result.risk_score,
            probability: result.wash_probability,
            auto_reject: result.auto_reject,
            reason,
            analysis_time_ms: result.analysis_time_ms,
        }
    }
}

impl Default for WashTradingScorer {
    fn default() -> Self {
        Self::new()
    }
}

/// Batch scoring API for analyzing multiple tokens
pub struct WashTradingBatchScorer {
    scorer: WashTradingScorer,
}

impl WashTradingBatchScorer {
    /// Create a new batch scorer
    pub fn new() -> Self {
        Self {
            scorer: WashTradingScorer::new(),
        }
    }

    /// Score multiple tokens in parallel
    ///
    /// # Arguments
    /// * `mints` - List of token mint addresses
    /// * `rpc_client` - Solana RPC client
    /// * `max_concurrent` - Maximum concurrent requests (default: 5)
    ///
    /// # Returns
    /// Vector of (mint, score) tuples, filtering out failed analyses
    pub async fn score_batch(
        &self,
        mints: &[&str],
        rpc_client: Arc<RpcClient>,
        max_concurrent: usize,
    ) -> Vec<(String, WashTradingScore)> {
        use futures::stream::{self, StreamExt};

        info!(
            "Batch scoring {} tokens with max_concurrent={}",
            mints.len(),
            max_concurrent
        );

        stream::iter(mints)
            .map(|&mint| {
                let scorer = self.scorer.clone();
                let rpc = rpc_client.clone();
                async move {
                    match scorer.score_token(mint, rpc).await {
                        Ok(score) => Some((mint.to_string(), score)),
                        Err(e) => {
                            warn!("Failed to score {}: {}", mint, e);
                            None
                        }
                    }
                }
            })
            .buffer_unordered(max_concurrent)
            .filter_map(|result| async move { result })
            .collect()
            .await
    }
}

impl Default for WashTradingBatchScorer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_scorer_creation() {
        let scorer = WashTradingScorer::new();
        // Just verify it was created successfully
        assert!(Arc::strong_count(&scorer.detector) > 0);
    }

    #[test]
    fn test_score_graph() {
        let config = GraphAnalyzerConfig::default();
        let mut graph = TransactionGraph::new(config);

        // Create circular pattern
        graph
            .add_transaction("w1", "w2", create_test_edge(1000))
            .unwrap();
        graph
            .add_transaction("w2", "w3", create_test_edge(1000))
            .unwrap();
        graph
            .add_transaction("w3", "w1", create_test_edge(1000))
            .unwrap();

        let scorer = WashTradingScorer::new();
        let score = scorer.score_graph(&graph).unwrap();

        assert!(score.risk_score >= 0 && score.risk_score <= 100);
        assert!(score.probability >= 0.0 && score.probability <= 1.0);
        assert!(!score.reason.is_empty());
    }

    #[test]
    fn test_score_empty_graph() {
        let config = GraphAnalyzerConfig::default();
        let graph = TransactionGraph::new(config);

        let scorer = WashTradingScorer::new();
        let score = scorer.score_graph(&graph).unwrap();

        assert_eq!(score.risk_score, 0);
        assert_eq!(score.probability, 0.0);
        assert!(!score.auto_reject);
        assert!(score.reason.contains("OK"));
    }

    #[test]
    fn test_batch_scorer_creation() {
        let batch_scorer = WashTradingBatchScorer::new();
        // Just verify it was created successfully
        assert!(Arc::strong_count(&batch_scorer.scorer.detector) > 0);
    }
}
