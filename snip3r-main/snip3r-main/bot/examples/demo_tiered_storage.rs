//! Example demonstrating the Tiered Storage Architecture
//!
//! This example shows how to use the Hot/Warm/Cold tiered storage system
//! for optimal performance and storage efficiency.
//!
//! Run with: cargo run --example demo_tiered_storage

use h_5n1p3r::oracle::{
    AutoTieringCoordinator, Outcome, ScoredCandidate, StorageTier, TieringConfig, TransactionRecord,
};
use h_5n1p3r::types::PremintCandidate;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Tiered Storage Architecture Demo ===\n");

    // Create a custom tiering configuration
    let config = TieringConfig {
        hot_to_warm_age: 300,     // 5 minutes
        warm_to_cold_age: 3600,   // 1 hour
        maintenance_interval: 60, // 1 minute
        auto_promote_on_access: true,
        min_hot_access_count: 2,
    };

    // Initialize the coordinator
    println!("Initializing AutoTieringCoordinator...");
    let coordinator = Arc::new(AutoTieringCoordinator::new(config).await?);
    println!("✓ Coordinator initialized\n");

    // Demonstrate storing records
    println!("--- Storing Records ---");
    let mut record_ids = Vec::new();

    for i in 0..10 {
        let record = create_sample_record(i, &format!("tx_signature_{}", i));
        let id = coordinator.put(&record).await?;
        record_ids.push(id);
        println!("✓ Stored record {} with ID {}", i, id);
    }
    println!();

    // Wait for cache to sync
    sleep(Duration::from_millis(100)).await;

    // Demonstrate retrieving records
    println!("--- Retrieving Records ---");

    // First access - should hit hot tier
    let start = std::time::Instant::now();
    let record = coordinator.get(record_ids[0]).await?;
    let duration = start.elapsed();
    println!(
        "✓ Retrieved record {} in {:?} (from hot tier)",
        record_ids[0], duration
    );
    assert!(record.is_some());

    // Access by signature - should also hit hot tier
    let start = std::time::Instant::now();
    let record = coordinator.get_by_signature("tx_signature_5").await?;
    let duration = start.elapsed();
    println!("✓ Retrieved by signature in {:?} (from hot tier)", duration);
    assert!(record.is_some());
    println!();

    // Display tier statistics
    println!("--- Tier Statistics ---");
    let stats = coordinator.get_stats().await?;
    println!("Hot Tier:  {} records", stats.hot_count);
    println!("Warm Tier: {} records", stats.warm_count);
    println!("Cold Tier: {} records", stats.cold_count);
    println!("Total:     {} records\n", stats.total_count());

    // Collect and display metrics
    println!("--- Performance Metrics ---");
    let metrics = coordinator.collect_metrics().await?;

    if let Some(hot) = metrics.hot {
        println!("Hot Tier Metrics:");
        println!("  Records: {}", hot.record_count);
        println!("  Hit Rate: {:.2}%", hot.hit_rate());
        println!("  Avg Access: {}µs", hot.avg_access_time_us);
        println!("  P95 Access: {}µs", hot.p95_access_time_us);
        println!("  P99 Access: {}µs", hot.p99_access_time_us);
        println!("  Bytes Stored: {:.2} KB", hot.bytes_stored as f64 / 1024.0);
        println!("  ✓ Within Target: {}", hot.is_within_target(1000));
        println!();
    }

    if let Some(warm) = metrics.warm {
        println!("Warm Tier Metrics:");
        println!("  Records: {}", warm.record_count);
        println!("  Hit Rate: {:.2}%", warm.hit_rate());
        println!("  Avg Access: {}µs", warm.avg_access_time_us);
        println!("  P95 Access: {}µs", warm.p95_access_time_us);
        println!("  P99 Access: {}µs", warm.p99_access_time_us);
        println!(
            "  Bytes Stored: {:.2} KB",
            warm.bytes_stored as f64 / 1024.0
        );
        println!("  ✓ Within Target: {}", warm.is_within_target(10_000));
        println!();
    }

    if let Some(cold) = metrics.cold {
        println!("Cold Tier Metrics:");
        println!("  Records: {}", cold.record_count);
        println!("  Hit Rate: {:.2}%", cold.hit_rate());
        println!(
            "  Bytes Stored: {:.2} KB",
            cold.bytes_stored as f64 / 1024.0
        );
        println!();
    }

    // Check for any alerts
    println!("--- Monitoring Alerts ---");
    let alerts = coordinator.get_alerts().await;
    if alerts.is_empty() {
        println!("✓ No alerts - all tiers performing within targets");
    } else {
        println!("⚠ Active alerts:");
        for alert in alerts {
            println!(
                "  [{:?}] {}: {}",
                alert.severity, alert.tier_name, alert.message
            );
        }
    }
    println!();

    // Demonstrate auto-promotion
    println!("--- Auto-Promotion Demo ---");

    // Create a record and store it only in warm tier
    let test_record = create_sample_record(100, "promoted_tx");
    let test_id = coordinator.warm_tier().put(&test_record).await?;
    println!("✓ Stored test record {} in warm tier only", test_id);

    // Access it through coordinator - should auto-promote to hot
    let _ = coordinator.get(test_id).await?;
    println!("✓ Accessed record through coordinator");

    // Wait for promotion
    sleep(Duration::from_millis(50)).await;

    // Check if it's now in hot tier
    let in_hot = coordinator.hot_tier().contains(test_id).await?;
    println!("✓ Record auto-promoted to hot tier: {}", in_hot);
    println!();

    // Run maintenance
    println!("--- Running Maintenance ---");
    coordinator.run_maintenance().await?;
    println!("✓ Maintenance completed\n");

    println!("=== Demo Complete ===");
    println!("\nKey Takeaways:");
    println!("1. Hot tier provides <1ms access for frequently used data");
    println!("2. Warm tier provides <10ms access with persistence");
    println!("3. Cold tier provides <100ms access for archival data");
    println!("4. Auto-tiering moves data automatically based on access patterns");
    println!("5. Monitoring tracks performance and alerts on threshold violations");

    Ok(())
}

/// Create a sample transaction record for testing
fn create_sample_record(id: i64, signature: &str) -> TransactionRecord {
    let mut feature_scores = HashMap::new();
    feature_scores.insert("liquidity".to_string(), 0.85);
    feature_scores.insert("holder_distribution".to_string(), 0.72);
    feature_scores.insert("transaction_velocity".to_string(), 0.68);

    let mut market_context = HashMap::new();
    market_context.insert("sol_price".to_string(), 100.0);
    market_context.insert("total_volume".to_string(), 50000.0);

    let scored_candidate = ScoredCandidate {
        base: PremintCandidate {
            mint: format!("mint_{}", id),
            creator: "creator_address".to_string(),
            program: "pump.fun".to_string(),
            slot: 12345 + id as u64,
            timestamp: chrono::Utc::now().timestamp() as u64,
            instruction_summary: Some("Test transaction".to_string()),
            is_jito_bundle: Some(false),
        },
        mint: format!("mint_{}", id),
        predicted_score: 85,
        reason: "High liquidity and good holder distribution".to_string(),
        feature_scores,
        calculation_time: 1500,
        anomaly_detected: false,
        timestamp: chrono::Utc::now().timestamp() as u64,
    };

    TransactionRecord {
        id: Some(id),
        scored_candidate,
        transaction_signature: Some(signature.to_string()),
        buy_price_sol: Some(0.001),
        sell_price_sol: Some(0.0012),
        amount_bought_tokens: Some(10000.0),
        amount_sold_tokens: Some(10000.0),
        initial_sol_spent: Some(1.0),
        final_sol_received: Some(1.2),
        timestamp_decision_made: chrono::Utc::now().timestamp() as u64,
        timestamp_transaction_sent: Some(chrono::Utc::now().timestamp() as u64 + 1),
        timestamp_outcome_evaluated: Some(chrono::Utc::now().timestamp() as u64 + 30),
        actual_outcome: Outcome::Profit(0.2),
        market_context_snapshot: market_context,
    }
}
