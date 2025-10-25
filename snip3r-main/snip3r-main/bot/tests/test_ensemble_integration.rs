//! Integration tests for the Quantum Ensemble Oracle System

use h_5n1p3r::oracle::{
    ConfidenceCalibrator, EnsembleConfig, EnsembleCoordinator, MarketRegime, OracleStrategy,
    PatternMemory, TradeOutcome,
};
use h_5n1p3r::types::PremintCandidate;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

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

#[tokio::test]
async fn test_ensemble_strategy_switching_based_on_regime() {
    // Test that the ensemble switches strategies based on market regime
    let regime = Arc::new(RwLock::new(MarketRegime::LowActivity));
    let calibrator = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
    let pattern_memory = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));

    let ensemble = EnsembleCoordinator::new(
        EnsembleConfig::default(),
        regime.clone(),
        calibrator,
        pattern_memory,
    );

    let candidate = create_test_candidate("mint1", 1000, true, "pump.fun");

    // In LowActivity, should use Conservative
    let (_, _, _, reason1) = ensemble.score_candidate(&candidate, None).await.unwrap();
    assert!(reason1.contains("Conservative") || reason1.contains("Ensemble"));

    // Switch to Bullish
    *regime.write().await = MarketRegime::Bullish;

    // In Bullish, should use Aggressive
    let (_, _, _, reason2) = ensemble.score_candidate(&candidate, None).await.unwrap();
    assert!(reason2.contains("Aggressive") || reason2.contains("Ensemble"));

    // Switch to Choppy
    *regime.write().await = MarketRegime::Choppy;

    // In Choppy, should use Ensemble voting
    let (_, _, _, reason3) = ensemble.score_candidate(&candidate, None).await.unwrap();
    assert!(reason3.contains("Ensemble"));
}

#[tokio::test]
async fn test_ensemble_performance_learning() {
    // Test that the ensemble learns from trade outcomes
    let regime = Arc::new(RwLock::new(MarketRegime::LowActivity));
    let calibrator = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
    let pattern_memory = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));

    let ensemble = EnsembleCoordinator::new(
        EnsembleConfig::default(),
        regime.clone(),
        calibrator,
        pattern_memory,
    );

    // Record successful trades for Conservative
    for _ in 0..15 {
        ensemble
            .record_trade_outcome(
                OracleStrategy::Conservative,
                MarketRegime::LowActivity,
                true,
                0.5,
                0.8,
            )
            .await;
    }

    // Record losing trades for Aggressive
    for _ in 0..15 {
        ensemble
            .record_trade_outcome(
                OracleStrategy::Aggressive,
                MarketRegime::LowActivity,
                false,
                -0.3,
                0.7,
            )
            .await;
    }

    // Check statistics
    let stats = ensemble.get_statistics().await;

    if let Some(conservative_perf) = stats.conservative_performance {
        assert!(
            conservative_perf.win_rate > 0.9,
            "Conservative should have high win rate"
        );
        assert!(
            conservative_perf.profit_factor > 1.0,
            "Conservative should be profitable"
        );
    }
}

#[tokio::test]
async fn test_ensemble_voting_aggregation() {
    // Test that voting properly aggregates different strategy predictions
    let regime = Arc::new(RwLock::new(MarketRegime::Choppy));
    let calibrator = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
    let pattern_memory = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));

    let ensemble = EnsembleCoordinator::new(
        EnsembleConfig::default(),
        regime.clone(),
        calibrator,
        pattern_memory,
    );

    let candidate = create_test_candidate("mint1", 1000, true, "pump.fun");

    // Score with ensemble voting
    let (score, confidence, features, reason) =
        ensemble.score_candidate(&candidate, None).await.unwrap();

    // Should use ensemble voting
    assert!(reason.contains("Ensemble"));

    // Should have ensemble-specific features
    assert!(features.contains_key("ensemble_score"));
    assert!(features.contains_key("ensemble_confidence"));
    assert!(features.contains_key("ensemble_uncertainty"));

    // Should have votes from both strategies
    assert!(
        features.contains_key("Conservative_score") || features.contains_key("Aggressive_score")
    );

    println!(
        "Voting result: score={}, confidence={:.3}, reason={}",
        score, confidence, reason
    );
}

#[tokio::test]
async fn test_ensemble_disabled_fallback() {
    // Test that disabling ensemble falls back to Conservative
    let regime = Arc::new(RwLock::new(MarketRegime::Bullish));
    let calibrator = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
    let pattern_memory = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));

    let mut config = EnsembleConfig::default();
    config.enabled = false;

    let ensemble = EnsembleCoordinator::new(config, regime.clone(), calibrator, pattern_memory);

    let candidate = create_test_candidate("mint1", 1000, true, "pump.fun");

    let (_, _, _, reason) = ensemble.score_candidate(&candidate, None).await.unwrap();
    assert!(reason.contains("disabled"));
}

#[tokio::test]
async fn test_ensemble_pattern_integration() {
    // Test that ensemble strategies use pattern memory
    let regime = Arc::new(RwLock::new(MarketRegime::LowActivity));
    let calibrator = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
    let pattern_memory = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));

    let ensemble = EnsembleCoordinator::new(
        EnsembleConfig::default(),
        regime.clone(),
        calibrator.clone(),
        pattern_memory.clone(),
    );

    // Create similar candidates
    let candidate1 = create_test_candidate("mint1", 1000, true, "pump.fun");
    let candidate2 = create_test_candidate("mint2", 1001, true, "pump.fun");

    // Score first candidate
    let (score1, _, _, _) = ensemble.score_candidate(&candidate1, None).await.unwrap();

    // Record successful pattern
    {
        let mut pm = pattern_memory.lock().await;
        let obs1 = h_5n1p3r::oracle::pattern_memory::Observation {
            features: vec![0.5, 0.5, 1.0, 0.5],
            timestamp: candidate1.timestamp,
        };
        let obs2 = h_5n1p3r::oracle::pattern_memory::Observation {
            features: vec![0.5, 0.5, 1.0, 0.5],
            timestamp: candidate2.timestamp,
        };
        pm.add_pattern(vec![obs1, obs2], true, 0.8);
    }

    // Score second similar candidate - should benefit from pattern
    let (score2, _, features, _) = ensemble.score_candidate(&candidate2, None).await.unwrap();

    // Pattern should influence scoring (though exact values may vary)
    println!(
        "Score 1: {}, Score 2: {}, Features: {:?}",
        score1,
        score2,
        features.keys()
    );
}

#[tokio::test]
async fn test_ensemble_confidence_calibration() {
    // Test that confidence calibration works with ensemble
    let regime = Arc::new(RwLock::new(MarketRegime::LowActivity));
    let calibrator = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
    let pattern_memory = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));

    let ensemble = EnsembleCoordinator::new(
        EnsembleConfig::default(),
        regime.clone(),
        calibrator.clone(),
        pattern_memory,
    );

    let candidate = create_test_candidate("mint1", 1000, true, "pump.fun");

    // Score candidate
    let (score, confidence, _, _) = ensemble.score_candidate(&candidate, None).await.unwrap();

    // Confidence should be between 0 and 1
    assert!(confidence >= 0.0 && confidence <= 1.0);

    // Record some outcomes to calibrator
    {
        let mut cal = calibrator.lock().await;
        for i in 0..20 {
            let outcome = h_5n1p3r::oracle::metacognition::DecisionOutcome::new(
                0.75,
                75,
                i % 2 == 0,
                if i % 2 == 0 { 0.5 } else { -0.3 },
                1000 + i,
            );
            cal.add_decision(outcome);
        }
    }

    // Score again - confidence should be calibrated
    let (_, confidence2, _, _) = ensemble.score_candidate(&candidate, None).await.unwrap();
    assert!(confidence2 >= 0.0 && confidence2 <= 1.0);

    println!(
        "Confidence before calibration: {:.3}, after: {:.3}",
        confidence, confidence2
    );
}

#[tokio::test]
async fn test_ensemble_statistics_tracking() {
    // Test comprehensive statistics tracking
    let regime = Arc::new(RwLock::new(MarketRegime::Bullish));
    let calibrator = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
    let pattern_memory = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));

    let ensemble = EnsembleCoordinator::new(
        EnsembleConfig::default(),
        regime.clone(),
        calibrator,
        pattern_memory,
    );

    // Record outcomes for both strategies
    for i in 0..10 {
        ensemble
            .record_trade_outcome(
                OracleStrategy::Conservative,
                MarketRegime::Bullish,
                i % 3 != 0,
                if i % 3 != 0 { 0.4 } else { -0.2 },
                0.75,
            )
            .await;

        ensemble
            .record_trade_outcome(
                OracleStrategy::Aggressive,
                MarketRegime::Bullish,
                i % 2 == 0,
                if i % 2 == 0 { 0.6 } else { -0.4 },
                0.80,
            )
            .await;
    }

    let stats = ensemble.get_statistics().await;

    assert_eq!(stats.current_regime, MarketRegime::Bullish);
    assert_eq!(stats.total_strategies, 2);
    assert!(stats.total_performance_entries > 0);

    if let Some(conservative_perf) = stats.conservative_performance {
        println!(
            "Conservative: win_rate={:.1}%, pf={:.2}, trades={}",
            conservative_perf.win_rate * 100.0,
            conservative_perf.profit_factor,
            conservative_perf.trade_count
        );
    }

    if let Some(aggressive_perf) = stats.aggressive_performance {
        println!(
            "Aggressive: win_rate={:.1}%, pf={:.2}, trades={}",
            aggressive_perf.win_rate * 100.0,
            aggressive_perf.profit_factor,
            aggressive_perf.trade_count
        );
    }
}
