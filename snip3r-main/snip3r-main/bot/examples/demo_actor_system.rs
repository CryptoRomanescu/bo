//! Example demonstrating the actor system foundation
//!
//! This example shows how to:
//! - Create and start actors
//! - Send messages between actors
//! - Query actor state
//! - Use the supervisor for fault tolerance

use actix::prelude::*;
use h_5n1p3r::actors::*;
use h_5n1p3r::oracle::quantum_oracle::SimpleOracleConfig;
use h_5n1p3r::types::{PremintCandidate, Pubkey};
use std::time::Duration;
use tokio::sync::mpsc;

#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Actor System Foundation Demo ===\n");

    // 1. Create configuration
    println!("1. Creating actor system configuration...");
    let oracle_config = SimpleOracleConfig::default();
    let rpc_url = "https://api.mainnet-beta.solana.com".to_string();
    let wallet_pubkey = Pubkey::try_from("11111111111111111111111111111112")?;
    let check_interval_ms = 1000;
    let (scored_sender, mut scored_receiver) = mpsc::channel(100);

    // 2. Create and start SupervisorActor
    println!("2. Starting SupervisorActor with ExponentialBackoff strategy...");
    let supervisor = SupervisorActor::new(
        SupervisionStrategy::ExponentialBackoff,
        oracle_config.clone(),
        rpc_url.clone(),
        wallet_pubkey.clone(),
        check_interval_ms,
        scored_sender.clone(),
    );

    let supervisor_addr = supervisor.start();
    println!("   ✓ SupervisorActor started\n");

    // 3. Create individual actors
    println!("3. Starting individual actors...");

    // Start OracleActor
    let oracle_actor = OracleActor::new(oracle_config, scored_sender)?;
    let oracle_addr = oracle_actor.start();
    println!("   ✓ OracleActor started");

    // Start StorageActor
    let storage_actor = StorageActor::new().await?;
    let storage_addr = storage_actor.start();
    println!("   ✓ StorageActor started\n");

    // 4. Query system health
    println!("4. Querying system health...");
    tokio::time::sleep(Duration::from_secs(1)).await;

    let health = supervisor_addr.send(GetSystemHealth).await?;
    println!("   System Health:");
    println!("   - Oracle healthy: {}", health.oracle_healthy);
    println!("   - Storage healthy: {}", health.storage_healthy);
    println!("   - Monitor healthy: {}", health.monitor_healthy);
    println!("   - Uptime: {} seconds\n", health.uptime_secs);

    // 5. Query Oracle metrics
    println!("5. Querying OracleActor metrics...");
    let metrics = oracle_addr.send(GetOracleMetrics).await?;
    println!("   Oracle Metrics:");
    println!("   - Total scored: {}", metrics.total_scored);
    println!("   - Avg scoring time: {:.2} ms", metrics.avg_scoring_time);
    println!("   - High score count: {}\n", metrics.high_score_count);

    // 6. Query Storage stats
    println!("6. Querying StorageActor stats...");
    let stats = storage_addr.send(GetStorageStats).await?;
    println!("   Storage Stats:");
    println!("   - Total decisions: {}", stats.total_decisions);
    println!("   - Pending outcomes: {}\n", stats.pending_outcomes);

    // 7. Send a candidate to score
    println!("7. Sending a test candidate to OracleActor...");
    let test_candidate = PremintCandidate {
        mint: Pubkey::try_from("11111111111111111111111111111113")?,
        creator: Pubkey::try_from("11111111111111111111111111111114")?,
        program: "pump.fun".to_string(),
        slot: 12345678,
        timestamp: chrono::Utc::now().timestamp_millis() as u64,
        instruction_summary: Some("Initialize token mint".to_string()),
        is_jito_bundle: Some(false),
    };

    let result = oracle_addr
        .send(ScoreCandidate {
            candidate: test_candidate,
        })
        .await?;

    match result {
        Ok(_) => println!("   ✓ Candidate sent for scoring\n"),
        Err(e) => println!("   ✗ Failed to score candidate: {}\n", e),
    }

    // 8. Wait a bit and check metrics again
    println!("8. Waiting and checking metrics again...");
    tokio::time::sleep(Duration::from_secs(2)).await;

    let metrics = oracle_addr.send(GetOracleMetrics).await?;
    println!("   Updated Oracle Metrics:");
    println!("   - Total scored: {}", metrics.total_scored);
    println!(
        "   - Avg scoring time: {:.2} ms\n",
        metrics.avg_scoring_time
    );

    // 9. Demonstrate fault tolerance
    println!("9. Demonstrating fault tolerance...");
    println!("   The SupervisorActor monitors all actors and will automatically");
    println!("   restart them if they crash (using the configured strategy).\n");

    // 10. Graceful shutdown
    println!("10. Initiating graceful shutdown...");
    supervisor_addr.send(ShutdownSystem).await?;
    tokio::time::sleep(Duration::from_secs(1)).await;
    println!("    ✓ System shut down gracefully\n");

    println!("=== Demo Complete ===");
    println!("\nKey Takeaways:");
    println!("✓ Actors provide isolation and fault tolerance");
    println!("✓ Message passing enables async communication");
    println!("✓ SupervisorActor manages actor lifecycle");
    println!("✓ System can be queried for health and metrics");

    Ok(())
}
