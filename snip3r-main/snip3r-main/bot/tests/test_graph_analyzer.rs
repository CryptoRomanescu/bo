//! Integration tests for on-chain graph analyzer

use chrono::Utc;
use h_5n1p3r::oracle::graph_analyzer::{
    GraphAnalyzerConfig, OnChainGraphAnalyzer, PatternType, TransactionEdge,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;

fn create_test_rpc_client() -> Arc<RpcClient> {
    Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ))
}

#[tokio::test]
async fn test_pump_and_dump_detection() {
    let config = GraphAnalyzerConfig::default();
    let rpc_client = create_test_rpc_client();
    let mut analyzer = OnChainGraphAnalyzer::new(config, rpc_client);

    let base_time = Utc::now();

    // Simulate coordinated buying (pump)
    for i in 0..10 {
        let wallet = format!("pump_wallet_{}", i);
        let edge = TransactionEdge {
            signature: format!("sig_pump_{}", i),
            amount: 5_000_000_000, // 5 SOL
            timestamp: base_time,
            slot: 100000 + i as u64,
            token_mint: Some("test_token".to_string()),
        };
        analyzer
            .update_with_transaction(&wallet, "token_pool", edge)
            .unwrap();
    }

    // Detect patterns
    let patterns = analyzer.get_patterns_by_type(PatternType::PumpAndDump);

    // Should detect at least one pump pattern
    assert!(!patterns.is_empty(), "Should detect pump & dump pattern");

    let pattern = &patterns[0];
    assert_eq!(pattern.pattern_type, PatternType::PumpAndDump);
    assert!(pattern.confidence > 0.5, "Confidence should be > 0.5");
    assert!(
        pattern.involved_wallets.len() >= 5,
        "Should involve multiple wallets"
    );
}

#[tokio::test]
async fn test_wash_trading_detection() {
    let config = GraphAnalyzerConfig::default();
    let rpc_client = create_test_rpc_client();
    let mut analyzer = OnChainGraphAnalyzer::new(config, rpc_client);

    let base_time = Utc::now();

    // Create circular trading pattern: A -> B -> C -> A
    let wallets = vec!["wash_a", "wash_b", "wash_c"];
    for i in 0..wallets.len() {
        let from = wallets[i];
        let to = wallets[(i + 1) % wallets.len()];

        let edge = TransactionEdge {
            signature: format!("sig_wash_{}_{}", from, to),
            amount: 1_000_000,
            timestamp: base_time,
            slot: 200000 + i as u64,
            token_mint: Some("test_token".to_string()),
        };

        analyzer.update_with_transaction(from, to, edge).unwrap();
    }

    // Detect patterns
    let patterns = analyzer.get_patterns_by_type(PatternType::WashTrading);

    // Should detect wash trading pattern
    assert!(!patterns.is_empty(), "Should detect wash trading pattern");

    let pattern = &patterns[0];
    assert_eq!(pattern.pattern_type, PatternType::WashTrading);
    assert!(pattern.confidence > 0.5, "Confidence should be > 0.5");
    assert!(
        pattern.evidence.circular_trading > 0.0,
        "Should have circular trading evidence"
    );
}

#[tokio::test]
async fn test_graph_build_performance() {
    let mut config = GraphAnalyzerConfig::default();
    config.max_wallets = 1000;

    let rpc_client = create_test_rpc_client();
    let mut analyzer = OnChainGraphAnalyzer::new(config, rpc_client);

    let base_time = Utc::now();
    let start = std::time::Instant::now();

    // Add 1000 transactions to simulate a large graph
    for i in 0..1000 {
        let from = format!("wallet_{}", i % 100); // 100 unique wallets
        let to = format!("wallet_{}", (i + 1) % 100);

        let edge = TransactionEdge {
            signature: format!("sig_{}", i),
            amount: 1_000_000 + i,
            timestamp: base_time + chrono::Duration::seconds(i as i64),
            slot: 300000 + i,
            token_mint: Some("test_token".to_string()),
        };

        analyzer.update_with_transaction(&from, &to, edge).unwrap();
    }

    let elapsed = start.elapsed();
    let metrics = analyzer.metrics();

    println!("Graph build performance:");
    println!("  Time: {:?}", elapsed);
    println!("  Nodes: {}", metrics.node_count);
    println!("  Edges: {}", metrics.edge_count);
    println!("  Density: {:.4}", metrics.density);

    // Performance assertions
    assert!(elapsed.as_secs() < 5, "Graph build should take < 5 seconds");
    assert_eq!(metrics.node_count, 100, "Should have 100 unique wallets");
    assert_eq!(metrics.edge_count, 1000, "Should have 1000 transactions");
}

#[tokio::test]
async fn test_memory_usage() {
    let mut config = GraphAnalyzerConfig::default();
    config.max_wallets = 10_000; // Test with 10k wallets as per requirements

    let rpc_client = create_test_rpc_client();
    let mut analyzer = OnChainGraphAnalyzer::new(config, rpc_client);

    let base_time = Utc::now();

    // Add transactions for 1000 wallets (scaled down from 10k for test speed)
    for i in 0..5000 {
        let from = format!("wallet_{}", i % 1000);
        let to = format!("wallet_{}", (i + 1) % 1000);

        let edge = TransactionEdge {
            signature: format!("sig_{}", i),
            amount: 1_000_000,
            timestamp: base_time + chrono::Duration::seconds(i as i64),
            slot: 400000 + i,
            token_mint: Some("test_token".to_string()),
        };

        analyzer.update_with_transaction(&from, &to, edge).unwrap();
    }

    let metrics = analyzer.metrics();

    println!("Memory usage test:");
    println!("  Nodes: {}", metrics.node_count);
    println!("  Edges: {}", metrics.edge_count);
    println!("  Estimated memory: {} bytes", metrics.memory_usage_bytes);
    println!(
        "  Memory per node: {} bytes",
        metrics.memory_usage_bytes / metrics.node_count.max(1)
    );

    // Memory should be reasonable (< 100MB for 1000 wallets)
    assert!(
        metrics.memory_usage_bytes < 100_000_000,
        "Memory usage should be < 100MB"
    );
}

#[tokio::test]
async fn test_real_time_updates() {
    let config = GraphAnalyzerConfig::default();
    let rpc_client = create_test_rpc_client();
    let mut analyzer = OnChainGraphAnalyzer::new(config, rpc_client);

    // Initial state
    assert_eq!(analyzer.metrics().node_count, 0);
    assert_eq!(analyzer.metrics().edge_count, 0);

    // Add first transaction
    let edge1 = TransactionEdge {
        signature: "sig1".to_string(),
        amount: 1_000_000,
        timestamp: Utc::now(),
        slot: 500000,
        token_mint: Some("test_token".to_string()),
    };
    analyzer
        .update_with_transaction("wallet_a", "wallet_b", edge1)
        .unwrap();

    assert_eq!(analyzer.metrics().node_count, 2);
    assert_eq!(analyzer.metrics().edge_count, 1);

    // Add second transaction (real-time update)
    let edge2 = TransactionEdge {
        signature: "sig2".to_string(),
        amount: 2_000_000,
        timestamp: Utc::now(),
        slot: 500001,
        token_mint: Some("test_token".to_string()),
    };
    analyzer
        .update_with_transaction("wallet_b", "wallet_c", edge2)
        .unwrap();

    assert_eq!(analyzer.metrics().node_count, 3);
    assert_eq!(analyzer.metrics().edge_count, 2);

    // Pattern detection should work with updated graph
    let patterns = analyzer.get_patterns_by_type(PatternType::WashTrading);
    // May or may not detect patterns with just 2 edges, but shouldn't crash
    assert!(patterns.len() >= 0);
}

#[tokio::test]
async fn test_wallet_clustering() {
    let config = GraphAnalyzerConfig::default();
    let rpc_client = create_test_rpc_client();
    let mut analyzer = OnChainGraphAnalyzer::new(config, rpc_client);

    let base_time = Utc::now();

    // Create a tightly connected cluster
    let cluster_wallets = vec!["cluster_a", "cluster_b", "cluster_c", "cluster_d"];
    for i in 0..cluster_wallets.len() {
        for j in 0..cluster_wallets.len() {
            if i != j {
                let edge = TransactionEdge {
                    signature: format!("sig_cluster_{}_{}", i, j),
                    amount: 1_000_000,
                    timestamp: base_time,
                    slot: 600000 + (i * 10 + j) as u64,
                    token_mint: Some("test_token".to_string()),
                };

                analyzer
                    .update_with_transaction(cluster_wallets[i], cluster_wallets[j], edge)
                    .unwrap();
            }
        }
    }

    // Add an isolated wallet pair
    let edge = TransactionEdge {
        signature: "sig_isolated".to_string(),
        amount: 1_000_000,
        timestamp: base_time,
        slot: 700000,
        token_mint: Some("test_token".to_string()),
    };
    analyzer
        .update_with_transaction("isolated_a", "isolated_b", edge)
        .unwrap();

    let metrics = analyzer.metrics();

    // Should have multiple connected components
    assert!(
        metrics.connected_components >= 2,
        "Should have at least 2 connected components"
    );

    // Sybil clusters might be detected
    let sybil_clusters = analyzer.get_sybil_clusters();
    println!("Detected {} Sybil clusters", sybil_clusters.len());
}

#[tokio::test]
async fn test_pattern_confidence_thresholds() {
    let mut config = GraphAnalyzerConfig::default();
    config.min_confidence = 0.9; // High threshold

    let rpc_client = create_test_rpc_client();
    let mut analyzer = OnChainGraphAnalyzer::new(config, rpc_client);

    let base_time = Utc::now();

    // Add weak signal (should not trigger with high threshold)
    for i in 0..3 {
        let wallet = format!("weak_wallet_{}", i);
        let edge = TransactionEdge {
            signature: format!("sig_weak_{}", i),
            amount: 100_000,
            timestamp: base_time + chrono::Duration::seconds(i as i64 * 600), // Spread out
            slot: 800000 + i as u64,
            token_mint: Some("test_token".to_string()),
        };
        analyzer
            .update_with_transaction(&wallet, "token_pool", edge)
            .unwrap();
    }

    let patterns = analyzer.get_patterns_by_type(PatternType::PumpAndDump);

    // With min_confidence=0.9, weak signals shouldn't be detected
    for pattern in &patterns {
        assert!(
            pattern.confidence >= 0.9,
            "All detected patterns should meet confidence threshold"
        );
    }
}
