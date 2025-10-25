//! Example demonstrating OpenTelemetry observability integration
//!
//! This example shows how to:
//! - Initialize observability with Jaeger tracing
//! - Create instrumented functions with spans
//! - Propagate context across async boundaries
//! - View traces in Jaeger UI
//!
//! # Setup
//!
//! 1. Start Jaeger:
//!    ```bash
//!    docker run -d --name jaeger \
//!      -e COLLECTOR_OTLP_ENABLED=true \
//!      -p 16686:16686 \
//!      -p 4317:4317 \
//!      jaegertracing/all-in-one:latest
//!    ```
//!
//! 2. Run this example:
//!    ```bash
//!    ENABLE_TRACING=true cargo run --example demo_observability
//!    ```
//!
//! 3. View traces in Jaeger UI:
//!    http://localhost:16686

use anyhow::Result;
use h_5n1p3r::observability::{init_observability, ObservabilityConfig};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, instrument, warn};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize observability with tracing enabled
    let config = ObservabilityConfig {
        service_name: "demo-observability".to_string(),
        enable_tracing: std::env::var("ENABLE_TRACING")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false),
        enable_metrics: true,
        sampling_ratio: 1.0, // Sample all traces for demo
        ..Default::default()
    };

    let _guard = init_observability(Some(config))?;

    info!("Starting observability demo");

    // Simulate some work with nested spans
    simulate_oracle_scoring().await?;
    simulate_pattern_matching().await?;
    simulate_transaction_monitoring().await?;

    info!("Demo completed - check Jaeger UI at http://localhost:16686");

    // Sleep briefly to allow traces to be exported
    sleep(Duration::from_secs(2)).await;

    Ok(())
}

/// Simulate oracle scoring with instrumentation
#[instrument(fields(mint = "DemoToken123"))]
async fn simulate_oracle_scoring() -> Result<()> {
    info!("Starting oracle scoring");

    // Simulate base scoring
    calculate_base_score().await?;

    // Simulate pattern lookup
    lookup_patterns().await?;

    // Simulate confidence calibration
    calibrate_confidence().await?;

    info!(score = 85, confidence = 0.9, "Scoring complete");
    Ok(())
}

/// Simulate base score calculation
#[instrument]
async fn calculate_base_score() -> Result<()> {
    info!("Calculating base score");
    sleep(Duration::from_millis(50)).await;
    info!(base_score = 70, "Base score calculated");
    Ok(())
}

/// Simulate pattern lookup
#[instrument]
async fn lookup_patterns() -> Result<()> {
    info!("Looking up historical patterns");
    sleep(Duration::from_millis(30)).await;
    info!(
        patterns_found = 3,
        best_match_confidence = 0.85,
        "Pattern lookup complete"
    );
    Ok(())
}

/// Simulate confidence calibration
#[instrument]
async fn calibrate_confidence() -> Result<()> {
    info!("Calibrating confidence");
    sleep(Duration::from_millis(20)).await;
    info!(calibrated_confidence = 0.9, "Confidence calibrated");
    Ok(())
}

/// Simulate pattern matching
#[instrument]
async fn simulate_pattern_matching() -> Result<()> {
    info!("Starting pattern matching");

    // Simulate extracting observations
    extract_observations().await?;

    // Simulate finding matches
    find_similar_patterns().await?;

    info!("Pattern matching complete");
    Ok(())
}

/// Simulate observation extraction
#[instrument]
async fn extract_observations() -> Result<()> {
    info!("Extracting observations from candidate");
    sleep(Duration::from_millis(15)).await;
    info!(observation_count = 5, "Observations extracted");
    Ok(())
}

/// Simulate pattern matching
#[instrument]
async fn find_similar_patterns() -> Result<()> {
    info!("Finding similar patterns in memory");
    sleep(Duration::from_millis(25)).await;
    info!(similarity_score = 0.87, "Pattern match found");
    Ok(())
}

/// Simulate transaction monitoring
#[instrument]
async fn simulate_transaction_monitoring() -> Result<()> {
    info!("Starting transaction monitoring");

    // Simulate checking transaction status
    check_transaction_status().await?;

    // Simulate outcome verification
    verify_outcome().await?;

    info!("Transaction monitoring complete");
    Ok(())
}

/// Simulate transaction status check
#[instrument(fields(signature = "DemoSig123"))]
async fn check_transaction_status() -> Result<()> {
    info!("Checking transaction status on-chain");
    sleep(Duration::from_millis(100)).await;
    info!(status = "confirmed", "Transaction confirmed");
    Ok(())
}

/// Simulate outcome verification
#[instrument]
async fn verify_outcome() -> Result<()> {
    info!("Verifying transaction outcome");
    sleep(Duration::from_millis(50)).await;

    // Simulate a warning for demonstration
    warn!(pnl_sol = -0.05, "Transaction resulted in loss");

    info!(outcome = "failure", "Outcome verified");
    Ok(())
}
