//! Demo of the Quantum Ensemble Oracle System
//!
//! This example demonstrates:
//! - ConservativeOracle and AggressiveOracle strategies
//! - MetaArbiter for dynamic strategy selection
//! - QuantumVoter for Bayesian voting
//! - Ensemble coordination and performance tracking

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use h_5n1p3r::oracle::{
    ConfidenceCalibrator, EnsembleConfig, EnsembleCoordinator, MarketRegime, OracleStrategy,
    PatternMemory, TradeOutcome,
};
use h_5n1p3r::types::PremintCandidate;

fn create_test_candidate(mint: &str, slot: u64, is_jito: bool, program: &str) -> PremintCandidate {
    PremintCandidate {
        mint: mint.to_string(),
        creator: "creator123".to_string(),
        program: program.to_string(),
        slot,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        instruction_summary: Some("create_token".to_string()),
        is_jito_bundle: Some(is_jito),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Quantum Ensemble Oracle System Demo ===\n");

    // Setup shared components
    let current_regime = Arc::new(RwLock::new(MarketRegime::LowActivity));
    let confidence_calibrator = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
    let pattern_memory = Arc::new(Mutex::new(PatternMemory::with_capacity(1000)));

    // Create ensemble coordinator
    let config = EnsembleConfig {
        enabled: true,
        voting_threshold: 0.6,
        strategy_switch_cooldown_secs: 60,
        performance_window_trades: 20,
        bayesian_prior_strength: 0.3,
    };

    let ensemble = EnsembleCoordinator::new(
        config,
        current_regime.clone(),
        confidence_calibrator,
        pattern_memory,
    );

    println!("✓ Initialized Ensemble Coordinator\n");

    // Demo 1: Score candidates in different market regimes
    println!("--- Demo 1: Scoring in Different Market Regimes ---\n");

    let candidates = vec![
        create_test_candidate("mint1", 1000, true, "pump.fun"),
        create_test_candidate("mint2", 1001, false, "raydium"),
        create_test_candidate("mint3", 1002, true, "unknown_program"),
    ];

    // Score in LowActivity regime (Conservative should be chosen)
    println!("Market Regime: LowActivity (Conservative expected)\n");
    for candidate in &candidates {
        let (score, confidence, features, reason) =
            ensemble.score_candidate(candidate, None).await?;

        println!("Candidate: {}", candidate.mint);
        println!("  Score: {}", score);
        println!("  Confidence: {:.3}", confidence);
        println!("  Reason: {}", reason);
        println!("  Features: {:?}", features.keys().collect::<Vec<_>>());
        println!();
    }

    // Switch to Bullish regime
    println!("--- Switching to Bullish Market Regime ---\n");
    *current_regime.write().await = MarketRegime::Bullish;

    println!("Market Regime: Bullish (Aggressive expected)\n");
    for candidate in &candidates {
        let (score, confidence, _, reason) = ensemble.score_candidate(candidate, None).await?;

        println!("Candidate: {}", candidate.mint);
        println!("  Score: {}", score);
        println!("  Confidence: {:.3}", confidence);
        println!("  Reason: {}", reason);
        println!();
    }

    // Demo 2: Record trade outcomes and observe performance tracking
    println!("--- Demo 2: Performance Tracking ---\n");

    // Simulate some trades with Conservative strategy
    println!("Recording Conservative strategy trades...");
    for i in 0..10 {
        let success = i % 3 != 0; // ~67% win rate
        let pnl = if success { 0.3 } else { -0.2 };

        ensemble
            .record_trade_outcome(
                OracleStrategy::Conservative,
                MarketRegime::LowActivity,
                success,
                pnl,
                0.75,
            )
            .await;
    }

    // Simulate some trades with Aggressive strategy
    println!("Recording Aggressive strategy trades...");
    for i in 0..10 {
        let success = i % 2 == 0; // ~50% win rate
        let pnl = if success { 0.5 } else { -0.3 };

        ensemble
            .record_trade_outcome(
                OracleStrategy::Aggressive,
                MarketRegime::Bullish,
                success,
                pnl,
                0.80,
            )
            .await;
    }

    // Get statistics
    let stats = ensemble.get_statistics().await;
    println!("\n✓ Recorded 20 trades total");
    println!(
        "  Total performance entries: {}",
        stats.total_performance_entries
    );

    if let Some(perf) = stats.conservative_performance {
        println!("\nConservative Strategy Performance:");
        println!("  Win Rate: {:.1}%", perf.win_rate * 100.0);
        println!("  Profit Factor: {:.2}", perf.profit_factor);
        println!("  Trade Count: {}", perf.trade_count);
    }

    if let Some(perf) = stats.aggressive_performance {
        println!("\nAggressive Strategy Performance:");
        println!("  Win Rate: {:.1}%", perf.win_rate * 100.0);
        println!("  Profit Factor: {:.2}", perf.profit_factor);
        println!("  Trade Count: {}", perf.trade_count);
    }

    // Demo 3: Ensemble voting in Choppy market
    println!("\n--- Demo 3: Ensemble Voting ---\n");

    *current_regime.write().await = MarketRegime::Choppy;

    println!("Market Regime: Choppy (Ensemble voting expected)\n");
    let test_candidate = create_test_candidate("mint_vote_test", 2000, true, "pump.fun");

    let (score, confidence, features, reason) =
        ensemble.score_candidate(&test_candidate, None).await?;

    println!("Ensemble Voting Result:");
    println!("  Final Score: {}", score);
    println!("  Final Confidence: {:.3}", confidence);
    println!("  Reason: {}", reason);

    if let Some(&uncertainty) = features.get("ensemble_uncertainty") {
        println!("  Uncertainty: {:.3}", uncertainty);
    }

    // Show individual strategy votes if available
    if let Some(&conservative_score) = features.get("Conservative_score") {
        println!("  Conservative Vote: {}", conservative_score as u8);
    }
    if let Some(&aggressive_score) = features.get("Aggressive_score") {
        println!("  Aggressive Vote: {}", aggressive_score as u8);
    }

    println!("\n=== Demo Complete ===");
    println!("\nKey Takeaways:");
    println!("✓ Ensemble adapts strategy based on market regime");
    println!("✓ MetaArbiter selects optimal strategy dynamically");
    println!("✓ Bayesian voting aggregates multiple predictions");
    println!("✓ Performance tracking enables continuous learning");

    Ok(())
}
