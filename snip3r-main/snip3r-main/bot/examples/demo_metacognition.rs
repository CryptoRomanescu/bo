//! Example: Demonstrating the Lightweight Metacognitive Awareness System
//!
//! This example shows how the ConfidenceCalibrator can be used to improve
//! decision-making in a trading bot by learning from historical outcomes.
//!
//! Run with: cargo run --example demo_metacognition

use h_5n1p3r::oracle::{ConfidenceCalibrator, DecisionOutcome};
use std::time::Instant;

fn main() {
    println!("=== Lightweight Metacognitive Awareness Demo ===\n");

    // Create a new confidence calibrator
    let mut calibrator = ConfidenceCalibrator::new();

    println!("1. Initial State");
    println!(
        "   Memory footprint: {}KB",
        calibrator.memory_footprint() / 1024
    );
    println!("   Decision count: {}\n", calibrator.decision_count());

    // Phase 1: Simulate early trading decisions (overconfident)
    println!("2. Phase 1: Early Trading (Overconfident)");
    println!(
        "   Simulating 30 decisions with 70% predicted confidence but only 40% success rate..."
    );

    for i in 0..30 {
        let outcome = DecisionOutcome::new(
            0.70,   // Predicted confidence: 70%
            70,     // Predicted score: 70/100
            i < 12, // Success: 40% (12 out of 30)
            if i < 12 { 0.4 } else { -0.3 },
            1000 + i,
        );
        calibrator.add_decision(outcome);
    }

    let phase1_stats = calibrator.get_stats();
    println!("   Results:");
    println!("     - Decisions: {}", phase1_stats.decision_count);
    println!(
        "     - Predicted confidence: {:.1}%",
        phase1_stats.avg_predicted_confidence * 100.0
    );
    println!(
        "     - Actual success rate: {:.1}%",
        phase1_stats.avg_success_rate * 100.0
    );
    println!(
        "     - Confidence bias: {:.3} (positive = overconfident)",
        phase1_stats.confidence_bias
    );
    println!(
        "     - Calibration error: {:.3}",
        phase1_stats.calibration_error
    );
    println!(
        "     - Calculation time: {}ns ({}μs)\n",
        phase1_stats.calculation_time_ns,
        phase1_stats.calculation_time_ns / 1000
    );

    // Demonstrate calibration adjustment
    println!("3. Confidence Calibration in Action");
    let raw_confidence = 0.70;
    let calibrated = calibrator.calibrate_confidence(raw_confidence);
    println!("   Raw confidence: {:.1}%", raw_confidence * 100.0);
    println!("   Calibrated confidence: {:.1}%", calibrated * 100.0);
    println!(
        "   Adjustment: {:.1}% (reduced due to overconfidence)\n",
        (raw_confidence - calibrated) * 100.0
    );

    // Phase 2: Apply calibration to improve decision-making
    println!("4. Phase 2: Trading with Calibration");
    println!("   Simulating 40 more decisions using calibrated confidence...");

    for i in 30..70 {
        // Use calibrated confidence for predictions
        let raw = 0.70;
        let calibrated_conf = calibrator.calibrate_confidence(raw);

        let outcome = DecisionOutcome::new(
            calibrated_conf, // Use calibrated value
            (calibrated_conf * 100.0) as u8,
            i < 55, // Success: 62.5% (25 out of 40)
            if i < 55 { 0.35 } else { -0.25 },
            1000 + i,
        );
        calibrator.add_decision(outcome);
    }

    let phase2_stats = calibrator.get_stats();
    println!("   Results:");
    println!("     - Total decisions: {}", phase2_stats.decision_count);
    println!(
        "     - Predicted confidence: {:.1}%",
        phase2_stats.avg_predicted_confidence * 100.0
    );
    println!(
        "     - Actual success rate: {:.1}%",
        phase2_stats.avg_success_rate * 100.0
    );
    println!(
        "     - Confidence bias: {:.3} (closer to 0 = better calibrated)",
        phase2_stats.confidence_bias
    );
    println!(
        "     - Calibration error: {:.3} (lower is better)",
        phase2_stats.calibration_error
    );
    println!(
        "     - Improvement: {:.3} reduction in bias\n",
        phase1_stats.confidence_bias - phase2_stats.confidence_bias
    );

    // Phase 3: Test with maximum capacity
    println!("5. Phase 3: Sliding Window (Maximum Capacity)");
    println!("   Adding 50 more decisions to exceed window size (100)...");

    for i in 70..120 {
        let outcome = DecisionOutcome::new(0.60, 60, i % 2 == 0, 0.2, 1000 + i);
        calibrator.add_decision(outcome);
    }

    println!(
        "   Window size: {} (automatically maintains sliding window)",
        calibrator.decision_count()
    );

    // Demonstrate performance
    println!("\n6. Performance Characteristics");
    let start = Instant::now();
    let stats = calibrator.get_stats();
    let elapsed = start.elapsed();

    println!(
        "   Memory footprint: {}KB ({} bytes)",
        calibrator.memory_footprint() / 1024,
        calibrator.memory_footprint()
    );
    println!(
        "   Calculation time: {}μs (requirement: <1000μs = <1ms)",
        elapsed.as_micros()
    );
    println!("   Stats calculation time: {}ns", stats.calculation_time_ns);
    println!("   Decisions tracked: {}", stats.decision_count);

    // Show recent decisions
    println!("\n7. Recent Decision History");
    let recent = calibrator.get_recent_decisions(5);
    println!("   Last 5 decisions:");
    for (idx, decision) in recent.iter().enumerate() {
        println!(
            "     {}. Confidence: {:.2}, Success: {}, Outcome: {:.2}",
            idx + 1,
            decision.predicted_confidence,
            if decision.was_successful {
                "✓"
            } else {
                "✗"
            },
            decision.actual_outcome
        );
    }

    println!("\n=== Demo Complete ===");
    println!("\nKey Takeaways:");
    println!("✓ Zero external dependencies (only std Rust)");
    println!(
        "✓ Memory efficient: ~{}KB per instance",
        calibrator.memory_footprint() / 1024
    );
    println!(
        "✓ CPU friendly: <1ms calculations (actual: {}μs)",
        elapsed.as_micros()
    );
    println!("✓ Real-time: Suitable for high-frequency trading");
    println!("✓ Adaptive: Learns from outcomes to improve calibration");
}
