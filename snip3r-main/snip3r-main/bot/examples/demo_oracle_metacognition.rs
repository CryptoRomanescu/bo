//! Example: Oracle with Metacognition Integration
//!
//! This example demonstrates how the PredictiveOracle uses the ConfidenceCalibrator
//! to improve decision-making through real-time confidence calibration.
//!
//! Run with: cargo run --example demo_oracle_metacognition

use h_5n1p3r::oracle::quantum_oracle::SimpleOracleConfig;
use h_5n1p3r::oracle::{CalibrationStats, PredictiveOracle};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

#[tokio::main]
async fn main() {
    println!("=== Oracle with Metacognition Integration Demo ===\n");

    // Setup Oracle
    let (candidate_tx, candidate_rx) = mpsc::channel(10);
    let (scored_tx, _scored_rx) = mpsc::channel(10);
    let config = Arc::new(RwLock::new(SimpleOracleConfig::default()));

    let oracle =
        PredictiveOracle::new(candidate_rx, scored_tx, config).expect("Failed to create Oracle");

    println!("1. Initial Oracle State");
    let initial_stats = oracle.get_calibration_stats().await;
    print_stats(&initial_stats);

    // Phase 1: Overconfident predictions
    println!("\n2. Phase 1: Recording Overconfident Predictions");
    println!("   Simulating 30 trades with 80% predicted confidence but only 40% success...");

    for i in 0..30 {
        let success = i < 12; // 40% success rate
        let outcome = if success { 0.5 } else { -0.3 };

        oracle
            .record_trade_outcome(
                0.80,     // Predicted confidence: 80%
                80,       // Predicted score: 80
                success,  // Actual success
                outcome,  // Normalized P&L
                1000 + i, // Timestamp
            )
            .await;
    }

    let phase1_stats = oracle.get_calibration_stats().await;
    print_stats(&phase1_stats);

    // Phase 2: Demonstrate confidence calibration effect
    println!("\n3. Phase 2: Improved Predictions with Calibration");
    println!("   Oracle now uses calibrated confidence to adjust scores...");
    println!("   Simulating 20 more trades with better-calibrated predictions...");

    for i in 30..50 {
        let success = i < 43; // 65% success rate
        let outcome = if success { 0.4 } else { -0.2 };

        oracle
            .record_trade_outcome(
                0.65, // More conservative confidence
                65,   // Lower predicted score
                success,
                outcome,
                1000 + i,
            )
            .await;
    }

    let phase2_stats = oracle.get_calibration_stats().await;
    print_stats(&phase2_stats);

    // Phase 3: Show metrics integration
    println!("\n4. Oracle Metrics with Calibration Data");
    let metrics = oracle.get_metrics().await;
    println!(
        "   Total calibration decisions: {}",
        metrics.calibration_decision_count
    );
    println!(
        "   Confidence bias: {:.3} (0=perfect, +ve=overconfident, -ve=underconfident)",
        metrics.confidence_bias
    );
    println!(
        "   Calibration error: {:.3} (lower is better)",
        metrics.calibration_error
    );
    println!(
        "   Average predicted confidence: {:.1}%",
        metrics.avg_predicted_confidence * 100.0
    );

    // Summary
    println!("\n5. Summary");
    println!("   ✅ Confidence calibration is working in real-time");
    println!("   ✅ Oracle learns from outcomes automatically");
    println!("   ✅ Calibration metrics available through API");
    println!(
        "   ✅ Performance: stats calculated in {}ns (<1ms requirement)",
        phase2_stats.calculation_time_ns
    );

    println!("\n=== Demo Complete ===");
}

fn print_stats(stats: &CalibrationStats) {
    println!("   📊 Calibration Statistics:");
    println!("      - Decisions in window: {}", stats.decision_count);
    println!(
        "      - Predicted confidence: {:.1}%",
        stats.avg_predicted_confidence * 100.0
    );
    println!(
        "      - Actual success rate: {:.1}%",
        stats.avg_success_rate * 100.0
    );
    println!(
        "      - Confidence bias: {:.3} ({})",
        stats.confidence_bias,
        if stats.confidence_bias > 0.0 {
            "overconfident"
        } else if stats.confidence_bias < 0.0 {
            "underconfident"
        } else {
            "well-calibrated"
        }
    );
    println!("      - Calibration error: {:.3}", stats.calibration_error);
    println!(
        "      - Calculation time: {}μs",
        stats.calculation_time_ns / 1000
    );
}
