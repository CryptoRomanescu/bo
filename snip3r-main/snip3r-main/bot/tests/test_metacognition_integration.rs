//! Integration test for Metacognition system in Oracle
//!
//! This test validates the complete integration of the ConfidenceCalibrator
//! into the Oracle scoring and outcome recording flow.

use h_5n1p3r::oracle::quantum_oracle::SimpleOracleConfig;
use h_5n1p3r::oracle::types::{FeatureWeights, ScoreThresholds};
use h_5n1p3r::oracle::{CalibrationStats, ConfidenceCalibrator, DecisionOutcome, PredictiveOracle};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_full_metacognition_integration() {
    // Setup Oracle
    let (candidate_tx, candidate_rx) = mpsc::channel(10);
    let (scored_tx, _scored_rx) = mpsc::channel(10);
    let config = Arc::new(RwLock::new(SimpleOracleConfig::default()));

    let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();

    // Phase 1: Record overconfident predictions
    for i in 0..20 {
        let success = i < 8; // 40% success rate
        let outcome = if success { 0.5 } else { -0.3 };
        oracle
            .record_trade_outcome(
                0.80, // Predicted confidence: 80%
                80,   // Predicted score: 80
                success,
                outcome,
                1000 + i,
            )
            .await;
    }

    // Verify calibration detects overconfidence
    let stats = oracle.get_calibration_stats().await;
    assert_eq!(stats.decision_count, 20);
    assert!((stats.avg_predicted_confidence - 0.80).abs() < 0.01);
    assert!((stats.avg_success_rate - 0.40).abs() < 0.01);
    assert!(stats.confidence_bias > 0.0, "Should detect overconfidence");
    assert!(
        stats.calibration_error > 0.0,
        "Should have calibration error"
    );

    // Phase 2: More accurate predictions
    for i in 20..35 {
        let success = i < 30; // 67% success rate
        let outcome = if success { 0.4 } else { -0.2 };
        oracle
            .record_trade_outcome(
                0.65, // Lower confidence
                65,
                success,
                outcome,
                1000 + i,
            )
            .await;
    }

    // Verify stats updated
    let stats = oracle.get_calibration_stats().await;
    assert_eq!(stats.decision_count, 35);

    // Phase 3: Verify metrics include calibration data
    let metrics = oracle.get_metrics().await;
    assert_eq!(metrics.calibration_decision_count, 35);
    assert!(metrics.confidence_bias != 0.0);
    assert!(metrics.calibration_error > 0.0);
    assert!(metrics.avg_predicted_confidence > 0.0);

    println!("✅ Full metacognition integration test passed!");
    println!("   Decision count: {}", metrics.calibration_decision_count);
    println!("   Confidence bias: {:.3}", metrics.confidence_bias);
    println!("   Calibration error: {:.3}", metrics.calibration_error);
    println!("   Avg confidence: {:.3}", metrics.avg_predicted_confidence);
}

#[tokio::test]
async fn test_calibration_improves_over_time() {
    // Setup Oracle
    let (candidate_tx, candidate_rx) = mpsc::channel(10);
    let (scored_tx, _scored_rx) = mpsc::channel(10);
    let config = Arc::new(RwLock::new(SimpleOracleConfig::default()));

    let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();

    // Phase 1: Poor calibration (overconfident)
    for i in 0..20 {
        oracle
            .record_trade_outcome(0.90, 90, i < 10, 0.0, 1000 + i)
            .await;
    }

    let stats1 = oracle.get_calibration_stats().await;
    let initial_error = stats1.calibration_error;

    // Phase 2: Better calibration
    for i in 20..40 {
        oracle
            .record_trade_outcome(0.60, 60, i < 32, 0.0, 1000 + i)
            .await;
    }

    let stats2 = oracle.get_calibration_stats().await;

    // Bias should be closer to zero after better-calibrated decisions
    assert!(
        stats2.confidence_bias.abs() < stats1.confidence_bias.abs(),
        "Bias should improve: {} vs {}",
        stats2.confidence_bias,
        stats1.confidence_bias
    );

    println!("✅ Calibration improvement test passed!");
    println!("   Initial error: {:.3}", initial_error);
    println!("   Final error: {:.3}", stats2.calibration_error);
    println!("   Initial bias: {:.3}", stats1.confidence_bias);
    println!("   Final bias: {:.3}", stats2.confidence_bias);
}

#[tokio::test]
async fn test_real_time_calibration_performance() {
    use std::time::Instant;

    // Setup Oracle
    let (candidate_tx, candidate_rx) = mpsc::channel(10);
    let (scored_tx, _scored_rx) = mpsc::channel(10);
    let config = Arc::new(RwLock::new(SimpleOracleConfig::default()));

    let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();

    // Fill calibrator to capacity
    for i in 0..100 {
        oracle
            .record_trade_outcome(0.75, 75, i % 2 == 0, 0.0, 1000 + i)
            .await;
    }

    // Measure performance of getting stats (should be <1ms)
    let start = Instant::now();
    let stats = oracle.get_calibration_stats().await;
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_micros() < 1000,
        "Should complete in <1ms, took {}μs",
        elapsed.as_micros()
    );
    assert_eq!(stats.decision_count, 100);
    assert!(
        stats.calculation_time_ns < 1_000_000,
        "Stats calculation should be <1ms"
    );

    println!("✅ Performance test passed!");
    println!("   Stats retrieval: {}μs", elapsed.as_micros());
    println!(
        "   Stats calculation: {}μs",
        stats.calculation_time_ns / 1000
    );
}

#[test]
fn test_calibration_stats_structure() {
    let mut calibrator = ConfidenceCalibrator::new();

    // Add diverse outcomes
    for i in 0..30 {
        let confidence = 0.60 + (i as f64 / 100.0);
        let success = i % 3 != 0; // 67% success
        let outcome = if success { 0.3 } else { -0.3 };

        calibrator.add_decision(DecisionOutcome::new(
            confidence,
            (confidence * 100.0) as u8,
            success,
            outcome,
            1000 + i as u64,
        ));
    }

    let stats = calibrator.get_stats();

    // Verify all fields are populated
    assert_eq!(stats.decision_count, 30);
    assert!(stats.accuracy > 0.0);
    assert!(stats.calibration_error >= 0.0);
    assert!(stats.avg_predicted_confidence > 0.0);
    assert!(stats.avg_success_rate > 0.0);
    // confidence_bias can be positive, negative, or zero
    assert!(stats.calculation_time_ns > 0);

    println!("✅ Calibration stats structure test passed!");
    println!("   All fields properly populated");
}
