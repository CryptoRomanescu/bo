//! Demo: On-Chain Graph Analyzer
//!
//! This example demonstrates how to use the on-chain graph analyzer
//! to detect pump & dump, wash trading, and wallet clustering patterns.

use chrono::Utc;
use h_5n1p3r::oracle::graph_analyzer::{
    GraphAnalyzerConfig, OnChainGraphAnalyzer, PatternType, TransactionEdge,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== On-Chain Graph Analyzer Demo ===\n");

    // Create RPC client
    let rpc_url = "https://api.mainnet-beta.solana.com".to_string();
    let rpc_client = Arc::new(RpcClient::new(rpc_url));

    // Configure graph analyzer
    let mut config = GraphAnalyzerConfig::default();
    config.max_wallets = 1000;
    config.max_transactions = 5000;
    config.min_confidence = 0.7;
    config.time_window_secs = 3600; // 1 hour

    println!("Configuration:");
    println!("  Max wallets: {}", config.max_wallets);
    println!("  Max transactions: {}", config.max_transactions);
    println!("  Min confidence: {}", config.min_confidence);
    println!("  Time window: {} seconds\n", config.time_window_secs);

    // Create analyzer
    let mut analyzer = OnChainGraphAnalyzer::new(config, rpc_client);

    // Demo 1: Build a synthetic transaction graph
    println!("Demo 1: Building synthetic transaction graph...");

    // Simulate pump & dump pattern
    let base_time = Utc::now();

    // Multiple wallets buying at the same time (pump)
    for i in 0..5 {
        let wallet = format!("PumpWallet{}", i);
        let edge = TransactionEdge {
            signature: format!("sig_pump_{}", i),
            amount: 10_000_000_000, // 10 SOL worth
            timestamp: base_time,
            slot: 100000 + i as u64,
            token_mint: Some("TokenMintExample".to_string()),
        };
        analyzer
            .update_with_transaction(&wallet, "TokenPool", edge)
            .unwrap();
    }

    // Then one wallet dumps (dump)
    let dump_time = base_time + chrono::Duration::seconds(300);
    let edge = TransactionEdge {
        signature: "sig_dump".to_string(),
        amount: 50_000_000_000, // 50 SOL worth
        timestamp: dump_time,
        slot: 100010,
        token_mint: Some("TokenMintExample".to_string()),
    };
    analyzer
        .update_with_transaction("TokenPool", "DumpWallet", edge)
        .unwrap();

    println!("  Added pump pattern: 5 wallets buying simultaneously");
    println!("  Added dump pattern: Large sell after 5 minutes\n");

    // Demo 2: Add wash trading pattern (circular transactions)
    println!("Demo 2: Adding wash trading pattern...");

    let wash_time = base_time + chrono::Duration::seconds(10);

    // Create circular trading: A -> B -> C -> A
    let edges = vec![
        ("WashA", "WashB", 1_000_000),
        ("WashB", "WashC", 1_000_000),
        ("WashC", "WashA", 1_000_000),
    ];

    for (from, to, amount) in edges {
        let edge = TransactionEdge {
            signature: format!("sig_wash_{}_{}", from, to),
            amount,
            timestamp: wash_time,
            slot: 100020,
            token_mint: Some("TokenMintExample".to_string()),
        };
        analyzer.update_with_transaction(from, to, edge).unwrap();
    }

    println!("  Added circular trading pattern: WashA -> WashB -> WashC -> WashA\n");

    // Demo 3: Get graph metrics
    println!("Demo 3: Graph Metrics");
    let metrics = analyzer.metrics();
    println!("  Nodes (wallets): {}", metrics.node_count);
    println!("  Edges (transactions): {}", metrics.edge_count);
    println!("  Density: {:.4}", metrics.density);
    println!("  Connected components: {}", metrics.connected_components);
    println!("  Build time: {}ms\n", metrics.build_time_ms);

    // Demo 4: Detect patterns
    println!("Demo 4: Pattern Detection");

    // In a real scenario, you would call:
    // let result = analyzer.analyze_token("actual_token_mint").await.unwrap();

    // For this demo, we'll detect patterns from our synthetic data
    // by accessing the internal graph (this would be done automatically in analyze_token)

    println!("  Note: In production, call analyzer.analyze_token(mint).await");
    println!("  This demo shows the structure without actual RPC calls\n");

    // Demo 5: Get patterns by type
    println!("Demo 5: Query Patterns by Type");
    let pump_patterns = analyzer.get_patterns_by_type(PatternType::PumpAndDump);
    let wash_patterns = analyzer.get_patterns_by_type(PatternType::WashTrading);

    println!("  Pump & Dump patterns detected: {}", pump_patterns.len());
    println!("  Wash Trading patterns detected: {}", wash_patterns.len());

    for pattern in &pump_patterns {
        println!("\n  Pump & Dump Pattern:");
        println!("    Confidence: {:.2}%", pattern.confidence * 100.0);
        println!("    Involved wallets: {}", pattern.involved_wallets.len());
        println!(
            "    Temporal clustering: {:.2}",
            pattern.evidence.temporal_clustering
        );
        println!("    Volume spike: {:.2}", pattern.evidence.volume_spike);
    }

    for pattern in &wash_patterns {
        println!("\n  Wash Trading Pattern:");
        println!("    Confidence: {:.2}%", pattern.confidence * 100.0);
        println!("    Involved wallets: {}", pattern.involved_wallets.len());
        println!(
            "    Circular trading score: {:.2}",
            pattern.evidence.circular_trading
        );
    }

    // Demo 6: Wallet clustering
    println!("\n\nDemo 6: Wallet Clustering");
    let sybil_clusters = analyzer.get_sybil_clusters();
    println!("  Sybil clusters detected: {}", sybil_clusters.len());

    for cluster in &sybil_clusters {
        println!("\n  Cluster #{}:", cluster.id);
        println!("    Wallets: {}", cluster.wallets.len());
        println!("    Cohesion: {:.2}", cluster.cohesion);
        println!("    Pattern: {:?}", cluster.pattern);
    }

    // Demo 7: Real-time updates
    println!("\n\nDemo 7: Real-time Update Capability");
    println!("  The analyzer supports real-time transaction updates:");
    println!("  - Call update_with_transaction() for new transactions");
    println!("  - Graph metrics are recalculated automatically");
    println!("  - Pattern detection can be re-run on demand");

    // Add a new transaction
    let new_edge = TransactionEdge {
        signature: "sig_realtime".to_string(),
        amount: 5_000_000,
        timestamp: Utc::now(),
        slot: 100100,
        token_mint: Some("TokenMintExample".to_string()),
    };
    analyzer
        .update_with_transaction("NewWallet", "ExistingWallet", new_edge)
        .unwrap();

    println!("\n  Added real-time transaction:");
    let updated_metrics = analyzer.metrics();
    println!("    Updated node count: {}", updated_metrics.node_count);
    println!("    Updated edge count: {}", updated_metrics.edge_count);

    // Demo 8: Feature extraction for ML
    println!("\n\nDemo 8: ML Feature Extraction");
    println!("  The graph analyzer provides 16 features for ML models:");
    println!("  - graph_pump_dump_count");
    println!("  - graph_wash_trading_count");
    println!("  - graph_coordinated_buying_count");
    println!("  - graph_sybil_cluster_count");
    println!("  - graph_risk_score (0.0-1.0)");
    println!("  - graph_is_suspicious (0/1)");
    println!("  - graph_node_count");
    println!("  - graph_edge_count");
    println!("  - graph_density");
    println!("  - graph_connected_components");
    println!("  - graph_avg_clustering_coef");
    println!("  - graph_max_pump_confidence");
    println!("  - graph_max_wash_confidence");
    println!("  - graph_avg_cluster_cohesion");
    println!("  - graph_max_cluster_size");
    println!("  - graph_build_time_ms");

    println!("\n\n=== Demo Complete ===");
    println!("\nKey Capabilities:");
    println!("  ✓ Transaction graph construction");
    println!("  ✓ Pump & dump detection");
    println!("  ✓ Wash trading detection");
    println!("  ✓ Wallet clustering");
    println!("  ✓ Sybil cluster detection");
    println!("  ✓ Real-time updates");
    println!("  ✓ ML feature integration");
    println!("\nPerformance Targets:");
    println!("  • Graph build time: <5 seconds for 10k wallets");
    println!("  • Pattern detection accuracy: >90%");
    println!("  • Memory usage: Optimized for 10k wallets");
}
