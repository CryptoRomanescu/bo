//! Integration tests for bot activity and bundle detection

use chrono::Utc;
use h_5n1p3r::oracle::bot_bundle_detector::{
    BotBundleConfig, BotBundleDetector, TokenClassification,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;

fn create_test_rpc_client() -> Arc<RpcClient> {
    Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ))
}

#[tokio::test]
async fn test_detector_creation() {
    let config = BotBundleConfig::default();
    let rpc_client = create_test_rpc_client();
    let _detector = BotBundleDetector::new(config, rpc_client);
}

#[tokio::test]
async fn test_token_classification_values() {
    // Test classification risk scores
    assert!(TokenClassification::Organic.risk_score() < 0.2);
    assert!(TokenClassification::Mixed.risk_score() < 0.5);
    assert!(TokenClassification::BotDominated.risk_score() > 0.6);
    assert!(TokenClassification::HighlyManipulated.risk_score() > 0.9);
    assert_eq!(TokenClassification::BundleCoordinated.risk_score(), 1.0);
}

#[tokio::test]
async fn test_classification_should_avoid() {
    assert!(!TokenClassification::Organic.should_avoid());
    assert!(!TokenClassification::Mixed.should_avoid());
    assert!(!TokenClassification::BotDominated.should_avoid());
    assert!(TokenClassification::HighlyManipulated.should_avoid());
    assert!(TokenClassification::BundleCoordinated.should_avoid());
}

#[tokio::test]
async fn test_config_defaults() {
    let config = BotBundleConfig::default();

    assert_eq!(config.min_transactions, 20);
    assert_eq!(config.healthy_organic_threshold, 0.30);
    assert_eq!(config.avoid_bot_threshold, 0.85);
    assert_eq!(config.bot_timing_threshold_ms, 100);
    assert_eq!(config.bundle_time_window_ms, 500);
    assert_eq!(config.min_bundle_size, 3);
    assert!(config.analysis_timeout.as_secs() == 10);
}

#[tokio::test]
async fn test_score_from_analysis() {
    use h_5n1p3r::oracle::bot_bundle_detector::{
        BotBundleAnalysis, BotBundleScore, BotMetrics, BundleMetrics,
    };

    // Create a bot-dominated analysis
    let analysis = BotBundleAnalysis {
        mint: "test_mint".to_string(),
        deploy_timestamp: 1000,
        analysis_timestamp: 2000,
        total_transactions: 100,
        bot_metrics: BotMetrics {
            bot_transaction_count: 85,
            unique_bot_addresses: 20,
            avg_bot_response_time_ms: 50.0,
            instant_transaction_percentage: 0.85,
            high_frequency_bots: 5,
        },
        bundle_metrics: BundleMetrics {
            bundle_count: 10,
            bundled_transaction_count: 30,
            bundle_percentage: 0.30,
            max_bundle_size: 5,
            avg_bundle_size: 3.0,
            coordinated_bundle_count: 8,
        },
        organic_ratio: 0.15,
        bot_percentage: 0.85,
        classification: TokenClassification::HighlyManipulated,
        suspicious_clusters: vec![],
        repeated_addresses: vec![],
        manipulation_score: 0.9,
        analysis_duration_ms: 5000,
    };

    let score = BotBundleScore::from_analysis(&analysis);

    assert_eq!(score.bot_penalty, 85);
    assert_eq!(score.bundle_penalty, 30);
    assert_eq!(score.organic_bonus, 15);
    assert_eq!(score.manipulation_score, 90);
    assert!(score.should_avoid);
    assert!(score.confidence > 0.5);
}

#[tokio::test]
async fn test_organic_analysis_scenario() {
    use h_5n1p3r::oracle::bot_bundle_detector::{
        BotBundleAnalysis, BotBundleScore, BotMetrics, BundleMetrics,
    };

    // Create an organic trading scenario
    let analysis = BotBundleAnalysis {
        mint: "organic_token".to_string(),
        deploy_timestamp: 1000,
        analysis_timestamp: 2000,
        total_transactions: 50,
        bot_metrics: BotMetrics {
            bot_transaction_count: 10,
            unique_bot_addresses: 8,
            avg_bot_response_time_ms: 150.0,
            instant_transaction_percentage: 0.20,
            high_frequency_bots: 1,
        },
        bundle_metrics: BundleMetrics {
            bundle_count: 2,
            bundled_transaction_count: 6,
            bundle_percentage: 0.12,
            max_bundle_size: 3,
            avg_bundle_size: 3.0,
            coordinated_bundle_count: 1,
        },
        organic_ratio: 0.80,
        bot_percentage: 0.20,
        classification: TokenClassification::Organic,
        suspicious_clusters: vec![],
        repeated_addresses: vec![],
        manipulation_score: 0.25,
        analysis_duration_ms: 3000,
    };

    let score = BotBundleScore::from_analysis(&analysis);

    assert!(
        score.bot_penalty < 30,
        "Bot penalty should be low for organic token"
    );
    assert!(!score.should_avoid, "Should not avoid organic token");
    assert!(score.organic_bonus > 50, "Organic bonus should be high");
}

#[tokio::test]
async fn test_bundle_coordinated_scenario() {
    use h_5n1p3r::oracle::bot_bundle_detector::{
        BotBundleAnalysis, BotBundleScore, BotMetrics, BundleMetrics,
    };

    // Create a bundle-coordinated pump scenario
    let analysis = BotBundleAnalysis {
        mint: "bundle_pump_token".to_string(),
        deploy_timestamp: 1000,
        analysis_timestamp: 2000,
        total_transactions: 100,
        bot_metrics: BotMetrics {
            bot_transaction_count: 40,
            unique_bot_addresses: 15,
            avg_bot_response_time_ms: 80.0,
            instant_transaction_percentage: 0.40,
            high_frequency_bots: 3,
        },
        bundle_metrics: BundleMetrics {
            bundle_count: 15,
            bundled_transaction_count: 60,
            bundle_percentage: 0.60,
            max_bundle_size: 8,
            avg_bundle_size: 4.0,
            coordinated_bundle_count: 12,
        },
        organic_ratio: 0.40,
        bot_percentage: 0.40,
        classification: TokenClassification::BundleCoordinated,
        suspicious_clusters: vec![],
        repeated_addresses: vec![],
        manipulation_score: 0.85,
        analysis_duration_ms: 4000,
    };

    let score = BotBundleScore::from_analysis(&analysis);

    assert!(score.bundle_penalty > 50, "Bundle penalty should be high");
    assert!(score.should_avoid, "Should avoid bundle-coordinated token");
    assert_eq!(
        analysis.classification.risk_score(),
        1.0,
        "Risk score should be maximum"
    );
}

#[tokio::test]
async fn test_detector_config_access() {
    let config = BotBundleConfig::default();
    let rpc_client = create_test_rpc_client();
    let detector = BotBundleDetector::new(config.clone(), rpc_client);

    let detector_config = detector.config();
    assert_eq!(detector_config.min_transactions, config.min_transactions);
    assert_eq!(
        detector_config.healthy_organic_threshold,
        config.healthy_organic_threshold
    );
}
