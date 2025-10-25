//! Graph Analysis Feature Extractor
//!
//! Extracts features from on-chain graph analysis for use in ML models

use crate::oracle::feature_engineering::catalog::FeatureCategory;
use crate::oracle::feature_engineering::extractors::{
    FeatureExtractor, FeatureValue, FeatureVector,
};
use crate::oracle::graph_analyzer::{GraphAnalyzerConfig, OnChainGraphAnalyzer, PatternType};
use crate::oracle::types_old::TokenData;
use crate::types::PremintCandidate;
use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Graph analysis feature extractor
pub struct GraphAnalysisExtractor {
    analyzer: Arc<RwLock<OnChainGraphAnalyzer>>,
}

impl GraphAnalysisExtractor {
    /// Create a new graph analysis extractor
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        let config = GraphAnalyzerConfig::default();
        let analyzer = OnChainGraphAnalyzer::new(config, rpc_client);

        Self {
            analyzer: Arc::new(RwLock::new(analyzer)),
        }
    }

    /// Create with custom configuration
    pub fn with_config(config: GraphAnalyzerConfig, rpc_client: Arc<RpcClient>) -> Self {
        let analyzer = OnChainGraphAnalyzer::new(config, rpc_client);

        Self {
            analyzer: Arc::new(RwLock::new(analyzer)),
        }
    }

    /// Extract features asynchronously
    pub async fn extract_async(
        &self,
        candidate: &PremintCandidate,
        _token_data: &TokenData,
    ) -> Result<FeatureVector> {
        let mut features = FeatureVector::new();

        // Analyze the token
        let mut analyzer = self.analyzer.write().await;
        let result = match analyzer.analyze_token(&candidate.mint).await {
            Ok(result) => result,
            Err(_) => {
                // If analysis fails, return zero features
                return Ok(self.default_features());
            }
        };

        // Pattern detection features
        features.insert(
            "graph_pump_dump_count".to_string(),
            result.summary.pump_dump_count as f64,
        );
        features.insert(
            "graph_wash_trading_count".to_string(),
            result.summary.wash_trading_count as f64,
        );
        features.insert(
            "graph_coordinated_buying_count".to_string(),
            result.summary.coordinated_buying_count as f64,
        );
        features.insert(
            "graph_sybil_cluster_count".to_string(),
            result.summary.sybil_cluster_count as f64,
        );
        features.insert("graph_risk_score".to_string(), result.summary.risk_score);
        features.insert(
            "graph_is_suspicious".to_string(),
            if result.summary.is_suspicious {
                1.0
            } else {
                0.0
            },
        );

        // Graph metrics features
        features.insert(
            "graph_node_count".to_string(),
            result.metrics.node_count as f64,
        );
        features.insert(
            "graph_edge_count".to_string(),
            result.metrics.edge_count as f64,
        );
        features.insert("graph_density".to_string(), result.metrics.density);
        features.insert(
            "graph_connected_components".to_string(),
            result.metrics.connected_components as f64,
        );
        features.insert(
            "graph_avg_clustering_coef".to_string(),
            result.metrics.avg_clustering_coefficient,
        );

        // Pattern confidence features
        let max_pump_confidence = result
            .patterns
            .iter()
            .filter(|p| p.pattern_type == PatternType::PumpAndDump)
            .map(|p| p.confidence)
            .fold(0.0_f64, f64::max);
        features.insert("graph_max_pump_confidence".to_string(), max_pump_confidence);

        let max_wash_confidence = result
            .patterns
            .iter()
            .filter(|p| p.pattern_type == PatternType::WashTrading)
            .map(|p| p.confidence)
            .fold(0.0_f64, f64::max);
        features.insert("graph_max_wash_confidence".to_string(), max_wash_confidence);

        // Cluster features
        let avg_cluster_cohesion = if !result.clusters.is_empty() {
            result.clusters.iter().map(|c| c.cohesion).sum::<f64>() / result.clusters.len() as f64
        } else {
            0.0
        };
        features.insert(
            "graph_avg_cluster_cohesion".to_string(),
            avg_cluster_cohesion,
        );

        let max_cluster_size = result
            .clusters
            .iter()
            .map(|c| c.wallets.len())
            .max()
            .unwrap_or(0) as f64;
        features.insert("graph_max_cluster_size".to_string(), max_cluster_size);

        // Performance metrics
        features.insert(
            "graph_build_time_ms".to_string(),
            result.metrics.build_time_ms as f64,
        );

        Ok(features)
    }

    /// Get default (zero) features when analysis fails
    fn default_features(&self) -> FeatureVector {
        let mut features = FeatureVector::new();

        features.insert("graph_pump_dump_count".to_string(), 0.0);
        features.insert("graph_wash_trading_count".to_string(), 0.0);
        features.insert("graph_coordinated_buying_count".to_string(), 0.0);
        features.insert("graph_sybil_cluster_count".to_string(), 0.0);
        features.insert("graph_risk_score".to_string(), 0.0);
        features.insert("graph_is_suspicious".to_string(), 0.0);
        features.insert("graph_node_count".to_string(), 0.0);
        features.insert("graph_edge_count".to_string(), 0.0);
        features.insert("graph_density".to_string(), 0.0);
        features.insert("graph_connected_components".to_string(), 0.0);
        features.insert("graph_avg_clustering_coef".to_string(), 0.0);
        features.insert("graph_max_pump_confidence".to_string(), 0.0);
        features.insert("graph_max_wash_confidence".to_string(), 0.0);
        features.insert("graph_avg_cluster_cohesion".to_string(), 0.0);
        features.insert("graph_max_cluster_size".to_string(), 0.0);
        features.insert("graph_build_time_ms".to_string(), 0.0);

        features
    }
}

impl FeatureExtractor for GraphAnalysisExtractor {
    fn extract(
        &self,
        candidate: &PremintCandidate,
        token_data: &TokenData,
    ) -> Result<FeatureVector> {
        // Since FeatureExtractor trait is synchronous but our analysis is async,
        // we return default features here. Users should use extract_async instead.
        // This maintains compatibility with the existing feature extraction pipeline.
        Ok(self.default_features())
    }

    fn category(&self) -> FeatureCategory {
        FeatureCategory::OnChain
    }

    fn feature_names(&self) -> Vec<String> {
        vec![
            "graph_pump_dump_count".to_string(),
            "graph_wash_trading_count".to_string(),
            "graph_coordinated_buying_count".to_string(),
            "graph_sybil_cluster_count".to_string(),
            "graph_risk_score".to_string(),
            "graph_is_suspicious".to_string(),
            "graph_node_count".to_string(),
            "graph_edge_count".to_string(),
            "graph_density".to_string(),
            "graph_connected_components".to_string(),
            "graph_avg_clustering_coef".to_string(),
            "graph_max_pump_confidence".to_string(),
            "graph_max_wash_confidence".to_string(),
            "graph_avg_cluster_cohesion".to_string(),
            "graph_max_cluster_size".to_string(),
            "graph_build_time_ms".to_string(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::types_old::*;
    use std::collections::VecDeque;

    fn create_test_rpc_client() -> Arc<RpcClient> {
        Arc::new(RpcClient::new(
            "https://api.mainnet-beta.solana.com".to_string(),
        ))
    }

    fn create_test_candidate() -> PremintCandidate {
        PremintCandidate {
            mint: "TestMint123456789".to_string(),
            creator: "TestCreator123456789".to_string(),
            program: "test".to_string(),
            slot: 12345,
            timestamp: 1640995200,
            instruction_summary: None,
            is_jito_bundle: Some(true),
        }
    }

    fn create_test_token_data() -> TokenData {
        TokenData {
            supply: 1_000_000_000,
            decimals: 9,
            metadata_uri: "https://example.com/metadata.json".to_string(),
            metadata: Some(Metadata {
                name: "Test Token".to_string(),
                symbol: "TEST".to_string(),
                description: "A test token".to_string(),
                image: "https://example.com/image.png".to_string(),
                attributes: vec![],
            }),
            holder_distribution: vec![],
            liquidity_pool: None,
            volume_data: VolumeData {
                initial_volume: 100.0,
                current_volume: 300.0,
                volume_growth_rate: 3.0,
                transaction_count: 50,
                buy_sell_ratio: 1.5,
            },
            creator_holdings: CreatorHoldings {
                initial_balance: 100_000_000,
                current_balance: 90_000_000,
                first_sell_timestamp: Some(1640995500),
                sell_transactions: 2,
            },
            holder_history: VecDeque::new(),
            price_history: VecDeque::new(),
            social_activity: SocialActivity {
                twitter_mentions: 50,
                telegram_members: 200,
                discord_members: 100,
                social_score: 0.7,
            },
        }
    }

    #[test]
    fn test_extractor_creation() {
        let rpc_client = create_test_rpc_client();
        let _extractor = GraphAnalysisExtractor::new(rpc_client);
    }

    #[test]
    fn test_feature_names() {
        let rpc_client = create_test_rpc_client();
        let extractor = GraphAnalysisExtractor::new(rpc_client);

        let names = extractor.feature_names();
        assert!(names.contains(&"graph_risk_score".to_string()));
        assert!(names.contains(&"graph_pump_dump_count".to_string()));
        assert_eq!(names.len(), 16);
    }

    #[test]
    fn test_category() {
        let rpc_client = create_test_rpc_client();
        let extractor = GraphAnalysisExtractor::new(rpc_client);

        assert_eq!(extractor.category(), FeatureCategory::OnChain);
    }

    #[test]
    fn test_extract_default_features() {
        let rpc_client = create_test_rpc_client();
        let extractor = GraphAnalysisExtractor::new(rpc_client);
        let candidate = create_test_candidate();
        let token_data = create_test_token_data();

        let features = extractor.extract(&candidate, &token_data).unwrap();

        // Should return default features
        assert!(features.contains_key("graph_risk_score"));
        assert_eq!(features.get("graph_risk_score"), Some(&0.0));
    }
}
