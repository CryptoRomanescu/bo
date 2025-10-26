//! Demo: Wash Trading Detector
//!
//! Demonstrates the wash trading detection capabilities with graph analysis.

use chrono::Utc;
use h_5n1p3r::oracle::graph_analyzer::{
    GraphAnalyzerConfig, OnChainGraphAnalyzer, TransactionEdge, TransactionGraph,
    WashTradingConfig, WashTradingDetector, WashTradingScorer,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("info,h_5n1p3r=debug")
        .init();

    println!("🔍 Wash Trading Detector Demo\n");

    // Create RPC client
    let rpc_url = "https://api.mainnet-beta.solana.com".to_string();
    let rpc_client = Arc::new(RpcClient::new(rpc_url));

    // Demo 1: Basic wash trading detection with synthetic data
    demo_basic_detection().await?;

    // Demo 2: Real-time wash trading scoring API
    demo_scoring_api().await?;

    // Demo 3: Integration with full graph analyzer
    demo_full_analyzer(rpc_client.clone()).await?;

    println!("\n✅ Demo completed successfully!");
    Ok(())
}

/// Demo 1: Basic detection with synthetic circular patterns
async fn demo_basic_detection() -> anyhow::Result<()> {
    println!("📊 Demo 1: Basic Wash Trading Detection");
    println!("─────────────────────────────────────────\n");

    // Create a graph with circular trading patterns
    let config = GraphAnalyzerConfig::default();
    let mut graph = TransactionGraph::new(config);

    // Scenario: Three wallets engaging in circular wash trading
    println!("Creating circular trading pattern: W1 → W2 → W3 → W1");

    // First cycle
    graph.add_transaction(
        "wallet_1",
        "wallet_2",
        TransactionEdge {
            signature: "sig_1".to_string(),
            amount: 1_000_000, // 1M tokens
            timestamp: Utc::now(),
            slot: 100,
            token_mint: Some("test_token".to_string()),
        },
    )?;

    graph.add_transaction(
        "wallet_2",
        "wallet_3",
        TransactionEdge {
            signature: "sig_2".to_string(),
            amount: 1_010_000, // Similar amount
            timestamp: Utc::now(),
            slot: 101,
            token_mint: Some("test_token".to_string()),
        },
    )?;

    graph.add_transaction(
        "wallet_3",
        "wallet_1",
        TransactionEdge {
            signature: "sig_3".to_string(),
            amount: 1_005_000, // Similar amount
            timestamp: Utc::now(),
            slot: 102,
            token_mint: Some("test_token".to_string()),
        },
    )?;

    // Second cycle (repeat the pattern)
    graph.add_transaction(
        "wallet_1",
        "wallet_2",
        TransactionEdge {
            signature: "sig_4".to_string(),
            amount: 990_000,
            timestamp: Utc::now(),
            slot: 103,
            token_mint: Some("test_token".to_string()),
        },
    )?;

    graph.add_transaction(
        "wallet_2",
        "wallet_3",
        TransactionEdge {
            signature: "sig_5".to_string(),
            amount: 995_000,
            timestamp: Utc::now(),
            slot: 104,
            token_mint: Some("test_token".to_string()),
        },
    )?;

    graph.add_transaction(
        "wallet_3",
        "wallet_1",
        TransactionEdge {
            signature: "sig_6".to_string(),
            amount: 1_000_000,
            timestamp: Utc::now(),
            slot: 105,
            token_mint: Some("test_token".to_string()),
        },
    )?;

    graph.calculate_metrics();

    // Run wash trading detection
    let detector = WashTradingDetector::default();
    let result = detector.detect(&graph)?;

    println!("Results:");
    println!(
        "  Wash Probability: {:.1}%",
        result.wash_probability * 100.0
    );
    println!("  Circularity Score: {:.2}", result.circularity_score);
    println!("  Risk Score: {}/100", result.risk_score);
    println!("  Circular Paths: {}", result.circular_paths.len());
    println!("  Suspicious Clusters: {}", result.suspicious_clusters);
    println!("  Auto-Reject: {}", result.auto_reject);
    println!("  Analysis Time: {}ms", result.analysis_time_ms);

    if !result.circular_paths.is_empty() {
        println!("\nDetected Circular Paths:");
        for (i, path) in result.circular_paths.iter().enumerate().take(3) {
            println!(
                "  Path {}: {} → ... (repeats {} times)",
                i + 1,
                path.path.first().unwrap_or(&"?".to_string()),
                path.repeat_count
            );
            println!("    Amount similarity: {:.2}", path.amount_similarity);
        }
    }

    println!();
    Ok(())
}

/// Demo 2: Using the scoring API
async fn demo_scoring_api() -> anyhow::Result<()> {
    println!("🎯 Demo 2: Wash Trading Scoring API");
    println!("─────────────────────────────────────────\n");

    // Create test graph with normal trading
    let config = GraphAnalyzerConfig::default();
    let mut graph = TransactionGraph::new(config);

    // Normal trading: W1 → W2, W3 → W4 (no circular pattern)
    println!("Creating normal trading pattern (non-circular)");

    graph.add_transaction(
        "buyer_1",
        "seller_1",
        TransactionEdge {
            signature: "normal_sig_1".to_string(),
            amount: 5_000_000,
            timestamp: Utc::now(),
            slot: 200,
            token_mint: Some("normal_token".to_string()),
        },
    )?;

    graph.add_transaction(
        "buyer_2",
        "seller_2",
        TransactionEdge {
            signature: "normal_sig_2".to_string(),
            amount: 3_000_000,
            timestamp: Utc::now(),
            slot: 201,
            token_mint: Some("normal_token".to_string()),
        },
    )?;

    graph.calculate_metrics();

    // Use scoring API
    let scorer = WashTradingScorer::new();
    let score = scorer.score_graph(&graph)?;

    println!("Scoring Results:");
    println!("  Risk Score: {}/100", score.risk_score);
    println!("  Probability: {:.1}%", score.probability * 100.0);
    println!("  Auto-Reject: {}", score.auto_reject);
    println!("  Reason: {}", score.reason);
    println!("  Analysis Time: {}ms", score.analysis_time_ms);

    println!();
    Ok(())
}

/// Demo 3: Integration with full graph analyzer
async fn demo_full_analyzer(rpc_client: Arc<RpcClient>) -> anyhow::Result<()> {
    println!("🌐 Demo 3: Full Graph Analyzer Integration");
    println!("─────────────────────────────────────────\n");

    let config = GraphAnalyzerConfig {
        max_transactions: 1000,
        time_window_secs: 3600,
        max_wallets: 500,
        min_confidence: 0.7,
        ..Default::default()
    };

    let mut analyzer = OnChainGraphAnalyzer::new(config, rpc_client);

    println!("Configuration:");
    println!("  Max Transactions: {}", analyzer.config().max_transactions);
    println!("  Time Window: {}s", analyzer.config().time_window_secs);
    println!("  Max Wallets: {}", analyzer.config().max_wallets);
    println!("  Min Confidence: {}", analyzer.config().min_confidence);

    // Note: In real usage, you would call:
    // let result = analyzer.analyze_token("token_mint_address").await?;
    // This would fetch real on-chain data and perform comprehensive analysis

    println!("\n  ℹ️  To analyze real tokens, use:");
    println!("     analyzer.analyze_token(\"<mint_address>\").await?");
    println!("     This performs full wash trading + pump detection + clustering");

    println!();
    Ok(())
}
