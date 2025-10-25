//! Demo of the Temporal Pattern Memory system
//!
//! This example demonstrates how to use the PatternMemory to:
//! 1. Record historical patterns with outcomes
//! 2. Find matching patterns for new observations
//! 3. Make predictions based on historical patterns
//!
//! Run with: cargo run --example demo_pattern_memory

use h_5n1p3r::oracle::{Observation, PatternMemory};

fn create_observation(liquidity: f32, volume_growth: f32, holder_growth: f32) -> Observation {
    Observation {
        features: vec![liquidity, volume_growth, holder_growth],
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    }
}

fn main() {
    println!("=== Temporal Pattern Memory Demo ===\n");

    // Create a pattern memory with capacity for 100 patterns
    let mut memory = PatternMemory::with_capacity(100);

    println!("1. Recording Historical Patterns");
    println!("---------------------------------");

    // Pattern 1: High liquidity + high growth -> Success (profit)
    let obs1_1 = create_observation(0.8, 0.9, 0.85);
    let obs1_2 = create_observation(0.85, 0.95, 0.9);
    memory.add_pattern(vec![obs1_1, obs1_2], true, 0.5);
    println!("✓ Added pattern: High liquidity + high growth -> Profit: 0.5 SOL");

    // Pattern 2: Similar to pattern 1 -> Success (profit)
    let obs2_1 = create_observation(0.75, 0.88, 0.82);
    let obs2_2 = create_observation(0.82, 0.92, 0.87);
    memory.add_pattern(vec![obs2_1, obs2_2], true, 0.6);
    println!("✓ Added pattern: High liquidity + high growth -> Profit: 0.6 SOL (similar to pattern 1, will merge)");

    // Pattern 3: Low liquidity + low growth -> Failure (loss)
    let obs3_1 = create_observation(0.2, 0.3, 0.25);
    let obs3_2 = create_observation(0.15, 0.25, 0.2);
    memory.add_pattern(vec![obs3_1, obs3_2], false, -0.3);
    println!("✓ Added pattern: Low liquidity + low growth -> Loss: -0.3 SOL");

    // Pattern 4: Medium liquidity + declining growth -> Failure (loss)
    let obs4_1 = create_observation(0.5, 0.6, 0.55);
    let obs4_2 = create_observation(0.48, 0.4, 0.45);
    memory.add_pattern(vec![obs4_1, obs4_2], false, -0.2);
    println!("✓ Added pattern: Medium liquidity + declining growth -> Loss: -0.2 SOL");

    // Pattern 5: Very high liquidity + explosive growth -> Big success
    let obs5_1 = create_observation(0.95, 0.85, 0.9);
    let obs5_2 = create_observation(0.98, 0.95, 0.95);
    memory.add_pattern(vec![obs5_1, obs5_2], true, 1.2);
    println!("✓ Added pattern: Very high liquidity + explosive growth -> Profit: 1.2 SOL");

    println!("\n2. Memory Statistics");
    println!("--------------------");
    let stats = memory.stats();
    println!("Total patterns stored: {}", stats.total_patterns);
    println!("Successful patterns: {}", stats.successful_patterns);
    println!("Success rate: {:.1}%", stats.success_rate * 100.0);
    println!("Average confidence: {:.2}", stats.avg_confidence);
    println!("Average PnL: {:.3} SOL", stats.avg_pnl);
    println!(
        "Memory usage: {} bytes ({:.2} KB)",
        stats.memory_usage_bytes,
        stats.memory_usage_bytes as f32 / 1024.0
    );

    println!("\n3. Pattern Matching and Prediction");
    println!("-----------------------------------");

    // Test case 1: New observation similar to successful pattern
    println!("\n[Test 1] New candidate with high liquidity + high growth:");
    let test1_1 = create_observation(0.78, 0.87, 0.83);
    let test1_2 = create_observation(0.84, 0.93, 0.88);

    match memory.find_match(&[test1_1, test1_2]) {
        Some(pattern_match) => {
            println!("  ✓ Match found!");
            println!("    Similarity: {:.1}%", pattern_match.similarity * 100.0);
            println!(
                "    Predicted outcome: {}",
                if pattern_match.predicted_outcome {
                    "SUCCESS"
                } else {
                    "FAILURE"
                }
            );
            println!("    Predicted PnL: {:.3} SOL", pattern_match.predicted_pnl);
            println!("    Confidence: {:.2}", pattern_match.pattern.confidence);
            println!(
                "    Pattern seen {} times",
                pattern_match.pattern.occurrence_count
            );
        }
        None => {
            println!("  ✗ No matching pattern found");
        }
    }

    // Test case 2: New observation similar to failed pattern
    println!("\n[Test 2] New candidate with low liquidity + low growth:");
    let test2_1 = create_observation(0.22, 0.28, 0.24);
    let test2_2 = create_observation(0.18, 0.23, 0.21);

    match memory.find_match(&[test2_1, test2_2]) {
        Some(pattern_match) => {
            println!("  ✓ Match found!");
            println!("    Similarity: {:.1}%", pattern_match.similarity * 100.0);
            println!(
                "    Predicted outcome: {}",
                if pattern_match.predicted_outcome {
                    "SUCCESS"
                } else {
                    "FAILURE"
                }
            );
            println!("    Predicted PnL: {:.3} SOL", pattern_match.predicted_pnl);
            println!("    Confidence: {:.2}", pattern_match.pattern.confidence);
            println!(
                "    Pattern seen {} times",
                pattern_match.pattern.occurrence_count
            );
        }
        None => {
            println!("  ✗ No matching pattern found");
        }
    }

    // Test case 3: New observation with no clear pattern
    println!("\n[Test 3] New candidate with mixed signals:");
    let test3_1 = create_observation(0.5, 0.2, 0.8);
    let test3_2 = create_observation(0.6, 0.1, 0.9);

    match memory.find_match(&[test3_1, test3_2]) {
        Some(pattern_match) => {
            println!("  ✓ Match found!");
            println!("    Similarity: {:.1}%", pattern_match.similarity * 100.0);
            println!(
                "    Predicted outcome: {}",
                if pattern_match.predicted_outcome {
                    "SUCCESS"
                } else {
                    "FAILURE"
                }
            );
            println!("    Predicted PnL: {:.3} SOL", pattern_match.predicted_pnl);
            println!("    Confidence: {:.2}", pattern_match.pattern.confidence);
        }
        None => {
            println!("  ✗ No matching pattern found (insufficient similarity or confidence)");
        }
    }

    println!("\n4. Learning from New Outcomes");
    println!("------------------------------");

    // Simulate adding a new observation that matches an existing pattern
    println!("Adding new outcome similar to successful pattern...");
    let new_obs1 = create_observation(0.77, 0.86, 0.81);
    let new_obs2 = create_observation(0.83, 0.91, 0.86);
    memory.add_pattern(vec![new_obs1, new_obs2], true, 0.55);

    let stats_after = memory.stats();
    println!("Updated statistics:");
    println!(
        "  Total patterns: {} (was {})",
        stats_after.total_patterns, stats.total_patterns
    );
    println!(
        "  Avg confidence: {:.2} (was {:.2})",
        stats_after.avg_confidence, stats.avg_confidence
    );
    println!(
        "  Avg PnL: {:.3} SOL (was {:.3} SOL)",
        stats_after.avg_pnl, stats.avg_pnl
    );

    println!("\n=== Demo Complete ===");
    println!("\nKey Takeaways:");
    println!("• Pattern memory consolidates similar patterns automatically");
    println!("• Confidence increases with consistent outcomes");
    println!(
        "• Memory usage is efficient: ~{:.2} KB for {} patterns",
        stats_after.memory_usage_bytes as f32 / 1024.0,
        stats_after.total_patterns
    );
    println!("• System learns and adapts from new observations");
}
