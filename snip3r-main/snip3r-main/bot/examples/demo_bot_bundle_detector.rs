//! Demo for bot activity and bundle detection
//!
//! This example demonstrates how to use the BotBundleDetector to analyze
//! Solana memecoin launches for bot activity and coordinated bundling.

use chrono::Utc;
use h_5n1p3r::oracle::bot_bundle_detector::{
    BotBundleAnalysis, BotBundleConfig, BotBundleDetector, BotBundleScore, BotMetrics,
    BundleMetrics, TokenClassification, TransactionTiming,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;

/// Create a simulated organic token scenario
fn create_organic_scenario() -> (Vec<TransactionTiming>, u64) {
    let deploy_timestamp = Utc::now().timestamp() as u64;
    let base_time = Utc::now();

    let mut transactions = Vec::new();

    // Simulate organic trading with diverse addresses and reasonable timing
    for i in 0..50 {
        let time_offset_ms = 5000 + i * 1000; // 5s+ between transactions
        transactions.push(TransactionTiming {
            signature: format!("sig_organic_{}", i),
            address: format!("human_wallet_{}", i),
            timestamp: base_time + chrono::Duration::milliseconds(time_offset_ms as i64),
            time_since_deploy_ms: time_offset_ms,
            slot: 1000000 + i,
            in_bundle: false,
            bundle_id: None,
        });
    }

    (transactions, deploy_timestamp)
}

/// Create a simulated bot-dominated scenario
fn create_bot_dominated_scenario() -> (Vec<TransactionTiming>, u64) {
    let deploy_timestamp = Utc::now().timestamp() as u64;
    let base_time = Utc::now();

    let mut transactions = Vec::new();

    // Simulate bot-dominated trading with fast response times
    for i in 0..100 {
        let time_offset_ms = if i < 85 {
            // 85% bots - very fast response
            50 + i
        } else {
            // 15% humans - slower response
            5000 + i * 100
        };

        transactions.push(TransactionTiming {
            signature: format!("sig_bot_{}", i),
            address: if i < 85 {
                format!("bot_wallet_{}", i)
            } else {
                format!("human_wallet_{}", i)
            },
            timestamp: base_time + chrono::Duration::milliseconds(time_offset_ms as i64),
            time_since_deploy_ms: time_offset_ms,
            slot: 1000000 + i,
            in_bundle: false,
            bundle_id: None,
        });
    }

    (transactions, deploy_timestamp)
}

/// Create a simulated bundle-coordinated scenario
fn create_bundle_coordinated_scenario() -> (Vec<TransactionTiming>, u64) {
    let deploy_timestamp = Utc::now().timestamp() as u64;
    let base_time = Utc::now();

    let mut transactions = Vec::new();
    let mut tx_id = 0;

    // Create 10 coordinated bundles, each with 5 transactions
    for bundle_num in 0..10 {
        let bundle_start_time = 1000 + bundle_num * 2000; // 2s between bundles

        for tx_in_bundle in 0..5 {
            let time_offset_ms = bundle_start_time + tx_in_bundle * 50; // 50ms within bundle

            transactions.push(TransactionTiming {
                signature: format!("sig_bundle_{}_{}", bundle_num, tx_in_bundle),
                address: format!("bundle_wallet_{}_{}", bundle_num, tx_in_bundle),
                timestamp: base_time + chrono::Duration::milliseconds(time_offset_ms as i64),
                time_since_deploy_ms: time_offset_ms,
                slot: 1000000 + tx_id,
                in_bundle: true,
                bundle_id: Some(bundle_num as usize),
            });

            tx_id += 1;
        }
    }

    (transactions, deploy_timestamp)
}

/// Analyze and print results for a scenario
fn analyze_scenario(
    name: &str,
    transactions: Vec<TransactionTiming>,
    deploy_timestamp: u64,
    detector: &BotBundleDetector,
) {
    println!("\n{}", "=".repeat(80));
    println!("Analyzing: {}", name);
    println!("{}", "=".repeat(80));
    println!("Total transactions: {}", transactions.len());

    // Perform analysis using the public API
    let analysis = detector
        .analyze_transactions(
            transactions,
            &format!("{}_token", name.to_lowercase().replace(' ', "_")),
            deploy_timestamp,
        )
        .expect("Analysis failed");

    // Print detailed results
    println!("\n--- Classification ---");
    println!("Type: {:?}", analysis.classification);
    println!("Description: {}", analysis.classification.description());
    println!("Risk Score: {:.2}", analysis.classification.risk_score());
    println!("Should Avoid: {}", analysis.classification.should_avoid());

    println!("\n--- Bot Metrics ---");
    println!(
        "Bot Transactions: {}/{} ({:.1}%)",
        analysis.bot_metrics.bot_transaction_count,
        analysis.total_transactions,
        analysis.bot_percentage * 100.0
    );
    println!(
        "Unique Bot Addresses: {}",
        analysis.bot_metrics.unique_bot_addresses
    );
    println!(
        "Avg Bot Response Time: {:.1}ms",
        analysis.bot_metrics.avg_bot_response_time_ms
    );
    println!(
        "Instant Tx %: {:.1}%",
        analysis.bot_metrics.instant_transaction_percentage * 100.0
    );
    println!(
        "High-Frequency Bots: {}",
        analysis.bot_metrics.high_frequency_bots
    );

    println!("\n--- Bundle Metrics ---");
    println!("Bundle Count: {}", analysis.bundle_metrics.bundle_count);
    println!(
        "Bundled Transactions: {}/{} ({:.1}%)",
        analysis.bundle_metrics.bundled_transaction_count,
        analysis.total_transactions,
        analysis.bundle_metrics.bundle_percentage * 100.0
    );
    println!(
        "Max Bundle Size: {}",
        analysis.bundle_metrics.max_bundle_size
    );
    println!(
        "Avg Bundle Size: {:.1}",
        analysis.bundle_metrics.avg_bundle_size
    );
    println!(
        "Coordinated Bundles: {}",
        analysis.bundle_metrics.coordinated_bundle_count
    );

    println!("\n--- Health Indicators ---");
    println!("Organic Ratio: {:.1}%", analysis.organic_ratio * 100.0);
    println!("Manipulation Score: {:.2}", analysis.manipulation_score);
    println!(
        "Suspicious Clusters: {}",
        analysis.suspicious_clusters.len()
    );
    println!("Repeated Addresses: {}", analysis.repeated_addresses.len());

    // Get score for decision engine
    let score = BotBundleScore::from_analysis(&analysis);

    println!("\n--- Decision Engine Score ---");
    println!("Bot Penalty: {}/100", score.bot_penalty);
    println!("Bundle Penalty: {}/100", score.bundle_penalty);
    println!("Organic Bonus: {}/100", score.organic_bonus);
    println!("Manipulation Score: {}/100", score.manipulation_score);
    println!("Should Avoid: {}", score.should_avoid);
    println!("Confidence: {:.2}", score.confidence);
    println!("Explanation: {}", score.explanation);

    println!("\n--- Recommendation ---");
    if score.should_avoid {
        println!("🚫 AVOID THIS TOKEN - High manipulation detected");
    } else if score.organic_bonus > 50 {
        println!("✅ HEALTHY TOKEN - Good organic trading activity");
    } else {
        println!("⚠️  CAUTION - Mixed signals, proceed with care");
    }
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║  Bot Activity & Bundle Detection Demo                         ║");
    println!("║  Universe-class Detection for Solana Memecoin Launches        ║");
    println!("╚════════════════════════════════════════════════════════════════╝");

    // Create detector
    let config = BotBundleConfig::default();
    let rpc_client = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let detector = BotBundleDetector::new(config.clone(), rpc_client);

    println!("\nConfiguration:");
    println!("- Min Transactions: {}", config.min_transactions);
    println!("- Analysis Window: {}s", config.analysis_window_secs);
    println!(
        "- Bot Timing Threshold: {}ms",
        config.bot_timing_threshold_ms
    );
    println!("- Bundle Time Window: {}ms", config.bundle_time_window_ms);
    println!(
        "- Healthy Organic Threshold: {:.0}%",
        config.healthy_organic_threshold * 100.0
    );
    println!(
        "- Avoid Bot Threshold: {:.0}%",
        config.avoid_bot_threshold * 100.0
    );

    // Scenario 1: Organic Token
    let (transactions, deploy_ts) = create_organic_scenario();
    analyze_scenario("Organic Token", transactions, deploy_ts, &detector);

    // Scenario 2: Bot-Dominated Token
    let (transactions, deploy_ts) = create_bot_dominated_scenario();
    analyze_scenario("Bot-Dominated Token", transactions, deploy_ts, &detector);

    // Scenario 3: Bundle-Coordinated Token
    let (transactions, deploy_ts) = create_bundle_coordinated_scenario();
    analyze_scenario(
        "Bundle-Coordinated Token",
        transactions,
        deploy_ts,
        &detector,
    );

    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║  Demo Complete                                                 ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");
}
