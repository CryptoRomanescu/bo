//! Integration tests for the Lightweight Metacognitive Awareness system

use h_5n1p3r::oracle::{ConfidenceCalibrator, DecisionOutcome};

#[test]
fn test_metacognition_full_workflow() {
    let mut calibrator = ConfidenceCalibrator::new();

    // Simulate a series of trading decisions with varying outcomes
    // First 20 decisions: overconfident (predict 0.8, get 0.5 success rate)
    for i in 0..20 {
        let outcome = DecisionOutcome::new(
            0.8, // High predicted confidence
            80,
            i < 10, // Only 50% succeed
            if i < 10 { 0.5 } else { -0.5 },
            1000 + i,
        );
        calibrator.add_decision(outcome);
    }

    // Get initial stats
    let stats = calibrator.get_stats();

    // Should detect overconfidence
    assert_eq!(stats.decision_count, 20);
    assert!((stats.avg_predicted_confidence - 0.8).abs() < 0.01);
    assert!((stats.avg_success_rate - 0.5).abs() < 0.01);
    assert!(stats.confidence_bias > 0.0, "Should be overconfident");

    // Test calibration adjustment
    let raw_confidence = 0.8;
    let calibrated = calibrator.calibrate_confidence(raw_confidence);
    assert!(
        calibrated < raw_confidence,
        "Calibration should reduce overconfidence"
    );

    println!("Initial Phase (Overconfident):");
    println!(
        "  Predicted confidence: {:.2}",
        stats.avg_predicted_confidence
    );
    println!("  Actual success rate: {:.2}", stats.avg_success_rate);
    println!("  Confidence bias: {:.3}", stats.confidence_bias);
    println!("  Calibrated {:.2} -> {:.2}", raw_confidence, calibrated);

    // Simulate improvement: Next 30 decisions with better performance
    for i in 20..50 {
        let outcome = DecisionOutcome::new(
            0.7, // More conservative confidence
            70,
            i < 45, // 83% success rate
            if i < 45 { 0.4 } else { -0.4 },
            1000 + i,
        );
        calibrator.add_decision(outcome);
    }

    let improved_stats = calibrator.get_stats();

    println!("\nImproved Phase:");
    println!(
        "  Predicted confidence: {:.2}",
        improved_stats.avg_predicted_confidence
    );
    println!(
        "  Actual success rate: {:.2}",
        improved_stats.avg_success_rate
    );
    println!("  Confidence bias: {:.3}", improved_stats.confidence_bias);
    println!(
        "  Calibration error: {:.3}",
        improved_stats.calibration_error
    );

    // Confidence bias should be smaller now (better calibrated)
    assert!(
        improved_stats.calibration_error < stats.calibration_error,
        "Calibration should improve over time"
    );
}

#[test]
fn test_metacognition_sliding_window_behavior() {
    let mut calibrator = ConfidenceCalibrator::new();

    // Fill beyond the window size
    for i in 0..150 {
        let outcome = DecisionOutcome::new(0.6, 60, true, 0.3, i);
        calibrator.add_decision(outcome);
    }

    // Should only keep last 100
    assert_eq!(calibrator.decision_count(), 100);

    // Recent decisions should be accessible
    let recent = calibrator.get_recent_decisions(10);
    assert_eq!(recent.len(), 10);

    // Most recent should have timestamp 149, then 148, etc.
    assert_eq!(recent[0].timestamp, 149);
    assert_eq!(recent[9].timestamp, 140);
}

#[test]
fn test_metacognition_real_world_scenario() {
    let mut calibrator = ConfidenceCalibrator::new();

    // Simulate a realistic trading bot scenario
    // Day 1: Learning phase (poor performance)
    for i in 0..25 {
        let outcome = DecisionOutcome::new(
            0.75, // Overconfident at start
            75,
            i % 3 == 0, // Only 33% success
            if i % 3 == 0 { 0.2 } else { -0.3 },
            1000 + i,
        );
        calibrator.add_decision(outcome);
    }

    let day1_stats = calibrator.get_stats();
    println!("Day 1 (Learning):");
    println!(
        "  Success rate: {:.1}%",
        day1_stats.avg_success_rate * 100.0
    );
    println!("  Confidence bias: {:.3}", day1_stats.confidence_bias);

    // Day 2: Improved with calibration
    for i in 25..50 {
        let raw_confidence = 0.75;
        let calibrated = calibrator.calibrate_confidence(raw_confidence);

        let outcome = DecisionOutcome::new(
            calibrated, // Use calibrated confidence
            (calibrated * 100.0) as u8,
            i % 2 == 0, // 50% success
            if i % 2 == 0 { 0.3 } else { -0.2 },
            1000 + i,
        );
        calibrator.add_decision(outcome);
    }

    let day2_stats = calibrator.get_stats();
    println!("\nDay 2 (Calibrated):");
    println!(
        "  Success rate: {:.1}%",
        day2_stats.avg_success_rate * 100.0
    );
    println!("  Confidence bias: {:.3}", day2_stats.confidence_bias);
    println!("  Calibration error: {:.3}", day2_stats.calibration_error);

    // Bias should be reduced
    assert!(
        day2_stats.confidence_bias.abs() < day1_stats.confidence_bias.abs(),
        "Calibration should reduce bias over time"
    );
}

#[test]
fn test_metacognition_performance_requirements() {
    let mut calibrator = ConfidenceCalibrator::new();

    // Fill to maximum capacity
    for i in 0..100 {
        let outcome = DecisionOutcome::new(0.7, 70, i % 2 == 0, 0.0, i);
        calibrator.add_decision(outcome);
    }

    // Test that statistics calculation is fast
    let start = std::time::Instant::now();
    let stats = calibrator.get_stats();
    let elapsed = start.elapsed();

    println!("Performance Test:");
    println!("  Window size: {}", stats.decision_count);
    println!("  Calculation time: {}μs", elapsed.as_micros());
    println!(
        "  Memory footprint: {}KB",
        calibrator.memory_footprint() / 1024
    );

    // Must be under 1ms (1000 microseconds)
    assert!(
        elapsed.as_micros() < 1000,
        "Calculation took {}μs, must be <1000μs",
        elapsed.as_micros()
    );

    // Memory footprint should be reasonable (5-10KB as specified)
    let footprint = calibrator.memory_footprint();
    assert!(
        footprint < 15000,
        "Memory footprint {}bytes exceeds 15KB limit",
        footprint
    );
}

#[test]
fn test_metacognition_clear_and_reset() {
    let mut calibrator = ConfidenceCalibrator::new();

    // Add some decisions
    for i in 0..30 {
        let outcome = DecisionOutcome::new(0.7, 70, true, 0.5, i);
        calibrator.add_decision(outcome);
    }

    assert_eq!(calibrator.decision_count(), 30);

    // Clear and verify reset
    calibrator.clear();

    assert_eq!(calibrator.decision_count(), 0);

    let stats = calibrator.get_stats();
    assert_eq!(stats.decision_count, 0);
    assert_eq!(stats.accuracy, 0.0);
}

#[test]
fn test_metacognition_confidence_bounds() {
    // Test that confidence values are properly clamped
    let outcome1 = DecisionOutcome::new(
        1.5, // Above 1.0
        120, // Above 100
        true, 2.0, // Above 1.0
        1000,
    );

    assert_eq!(outcome1.predicted_confidence, 1.0);
    assert_eq!(outcome1.predicted_score, 100);
    assert_eq!(outcome1.actual_outcome, 1.0);

    let outcome2 = DecisionOutcome::new(
        -0.5, // Below 0.0
        0, false, -2.0, // Below -1.0
        1000,
    );

    assert_eq!(outcome2.predicted_confidence, 0.0);
    assert_eq!(outcome2.actual_outcome, -1.0);

    // Test calibrator bounds
    let mut calibrator = ConfidenceCalibrator::new();

    // Add some history
    for i in 0..20 {
        let outcome = DecisionOutcome::new(0.5, 50, true, 0.5, i);
        calibrator.add_decision(outcome);
    }

    // Test edge cases
    let calibrated_high = calibrator.calibrate_confidence(1.5);
    let calibrated_low = calibrator.calibrate_confidence(-0.5);

    assert!(calibrated_high >= 0.0 && calibrated_high <= 1.0);
    assert!(calibrated_low >= 0.0 && calibrated_low <= 1.0);
}
