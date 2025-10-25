//! Quantum Ensemble Oracle System
//!
//! This module implements a sophisticated ensemble system that combines multiple
//! oracle strategies through intelligent voting and meta-arbitration for enhanced
//! accuracy, robustness, and adaptability.

pub mod meta_arbiter;
pub mod quantum_voter;
pub mod strategies;
pub mod types;

// Re-export main types and components
pub use types::{
    ArbiterDecision, EnsembleConfig, OracleStrategy, StrategyConfig, StrategyPerformance,
    VotingResult,
};

pub use strategies::{AggressiveOracle, ConservativeOracle, OracleStrategyTrait};

pub use meta_arbiter::{MetaArbiter, TradeOutcome};

pub use quantum_voter::QuantumVoter;

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info};

use crate::oracle::metacognition::ConfidenceCalibrator;
use crate::oracle::pattern_memory::PatternMemory;
use crate::oracle::types::MarketRegime;
use crate::types::PremintCandidate;

/// Ensemble coordinator that manages multiple oracle strategies
pub struct EnsembleCoordinator {
    /// Ensemble configuration
    config: EnsembleConfig,

    /// Conservative oracle strategy
    conservative: ConservativeOracle,

    /// Aggressive oracle strategy
    aggressive: AggressiveOracle,

    /// Meta arbiter for strategy selection
    meta_arbiter: Arc<Mutex<MetaArbiter>>,

    /// Quantum voter for Bayesian voting
    quantum_voter: QuantumVoter,

    /// Current market regime (shared)
    current_regime: Arc<RwLock<MarketRegime>>,
}

impl EnsembleCoordinator {
    /// Create a new ensemble coordinator
    pub fn new(
        config: EnsembleConfig,
        current_regime: Arc<RwLock<MarketRegime>>,
        confidence_calibrator: Arc<Mutex<ConfidenceCalibrator>>,
        pattern_memory: Arc<Mutex<PatternMemory>>,
    ) -> Self {
        info!("Initializing Quantum Ensemble Oracle System");

        let conservative =
            ConservativeOracle::new(confidence_calibrator.clone(), pattern_memory.clone());

        let aggressive = AggressiveOracle::new(confidence_calibrator, pattern_memory);

        let meta_arbiter = Arc::new(Mutex::new(MetaArbiter::new(
            current_regime.clone(),
            config.clone(),
        )));

        let quantum_voter = QuantumVoter::new(config.clone());

        Self {
            config,
            conservative,
            aggressive,
            meta_arbiter,
            quantum_voter,
            current_regime,
        }
    }

    /// Score a candidate using the ensemble system
    ///
    /// This will either:
    /// 1. Use a single strategy selected by MetaArbiter
    /// 2. Use Bayesian voting to aggregate multiple strategies
    #[tracing::instrument(skip(self, candidate, historical_data), fields(mint = %candidate.mint))]
    pub async fn score_candidate(
        &self,
        candidate: &PremintCandidate,
        historical_data: Option<&[PremintCandidate]>,
    ) -> Result<(u8, f64, HashMap<String, f64>, String)> {
        if !self.config.enabled {
            // Ensemble disabled, use conservative by default
            let (score, conf, features) = self
                .conservative
                .score_candidate(candidate, historical_data)
                .await?;
            return Ok((
                score,
                conf,
                features,
                "Ensemble disabled, using Conservative".to_string(),
            ));
        }

        // Get strategy decision from MetaArbiter
        let arbiter = self.meta_arbiter.lock().await;
        let decision = arbiter.get_strategy().await;
        drop(arbiter); // Release lock early

        debug!("Ensemble decision: {:?}", decision.selected_strategy);

        match decision.selected_strategy {
            OracleStrategy::Conservative => {
                let (score, conf, features) = self
                    .conservative
                    .score_candidate(candidate, historical_data)
                    .await?;
                Ok((score, conf, features, decision.reason))
            }
            OracleStrategy::Aggressive => {
                let (score, conf, features) = self
                    .aggressive
                    .score_candidate(candidate, historical_data)
                    .await?;
                Ok((score, conf, features, decision.reason))
            }
            OracleStrategy::Ensemble => {
                // Use voting
                self.score_with_voting(candidate, historical_data, &decision)
                    .await
            }
        }
    }

    /// Score using Bayesian voting from multiple strategies
    async fn score_with_voting(
        &self,
        candidate: &PremintCandidate,
        historical_data: Option<&[PremintCandidate]>,
        decision: &ArbiterDecision,
    ) -> Result<(u8, f64, HashMap<String, f64>, String)> {
        debug!("Using Bayesian voting for candidate: {}", candidate.mint);

        // Get predictions from all strategies
        let mut predictions = HashMap::new();

        let (c_score, c_conf, c_features) = self
            .conservative
            .score_candidate(candidate, historical_data)
            .await?;
        predictions.insert(OracleStrategy::Conservative, (c_score, c_conf, c_features));

        let (a_score, a_conf, a_features) = self
            .aggressive
            .score_candidate(candidate, historical_data)
            .await?;
        predictions.insert(OracleStrategy::Aggressive, (a_score, a_conf, a_features));

        // Get performance metrics
        let arbiter = self.meta_arbiter.lock().await;
        let regime = decision.market_regime;
        let mut performance_metrics = HashMap::new();

        if let Some(perf) = arbiter.get_performance(OracleStrategy::Conservative, regime) {
            performance_metrics.insert(OracleStrategy::Conservative, perf.clone());
        }
        if let Some(perf) = arbiter.get_performance(OracleStrategy::Aggressive, regime) {
            performance_metrics.insert(OracleStrategy::Aggressive, perf.clone());
        }
        drop(arbiter);

        // Perform Bayesian voting
        let voting_result = self
            .quantum_voter
            .vote(predictions.clone(), &performance_metrics)?;

        // Aggregate feature scores
        let mut aggregated_features = HashMap::new();
        aggregated_features.insert(
            "ensemble_score".to_string(),
            voting_result.final_score as f64,
        );
        aggregated_features.insert(
            "ensemble_confidence".to_string(),
            voting_result.final_confidence,
        );
        aggregated_features.insert(
            "ensemble_uncertainty".to_string(),
            voting_result.uncertainty,
        );

        // Add individual strategy scores
        for (strategy, (score, conf)) in &voting_result.strategy_votes {
            aggregated_features.insert(format!("{}_score", strategy), *score as f64);
            aggregated_features.insert(format!("{}_confidence", strategy), *conf);
        }

        info!(
            "Ensemble voting result: score={}, confidence={:.3}, uncertainty={:.3}",
            voting_result.final_score, voting_result.final_confidence, voting_result.uncertainty
        );

        Ok((
            voting_result.final_score,
            voting_result.final_confidence,
            aggregated_features,
            voting_result.reason,
        ))
    }

    /// Record a trade outcome for learning
    pub async fn record_trade_outcome(
        &self,
        strategy: OracleStrategy,
        regime: MarketRegime,
        was_successful: bool,
        pnl: f64,
        confidence: f64,
    ) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let outcome = TradeOutcome {
            strategy,
            regime,
            was_successful,
            pnl,
            confidence,
            timestamp,
        };

        let mut arbiter = self.meta_arbiter.lock().await;
        arbiter.record_trade_outcome(outcome).await;
    }

    /// Get ensemble statistics
    pub async fn get_statistics(&self) -> EnsembleStats {
        let arbiter = self.meta_arbiter.lock().await;
        let all_performance = arbiter.get_all_performance();

        let regime = *self.current_regime.read().await;

        let conservative_perf = arbiter
            .get_performance(OracleStrategy::Conservative, regime)
            .cloned();
        let aggressive_perf = arbiter
            .get_performance(OracleStrategy::Aggressive, regime)
            .cloned();

        EnsembleStats {
            current_regime: regime,
            total_strategies: 2,
            conservative_performance: conservative_perf,
            aggressive_performance: aggressive_perf,
            total_performance_entries: all_performance.len(),
        }
    }
}

/// Statistics for the ensemble system
#[derive(Debug, Clone)]
pub struct EnsembleStats {
    pub current_regime: MarketRegime,
    pub total_strategies: usize,
    pub conservative_performance: Option<StrategyPerformance>,
    pub aggressive_performance: Option<StrategyPerformance>,
    pub total_performance_entries: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_candidate(mint: &str, slot: u64, is_jito: bool) -> PremintCandidate {
        PremintCandidate {
            mint: mint.to_string(),
            creator: "creator123".to_string(),
            program: "pump.fun".to_string(),
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
    async fn test_ensemble_coordinator_creation() {
        let config = EnsembleConfig::default();
        let regime = Arc::new(RwLock::new(MarketRegime::LowActivity));
        let calibrator = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
        let pattern_memory = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));

        let coordinator = EnsembleCoordinator::new(config, regime, calibrator, pattern_memory);

        assert!(coordinator.config.enabled);
    }

    #[tokio::test]
    async fn test_score_candidate_with_ensemble() {
        let config = EnsembleConfig::default();
        let regime = Arc::new(RwLock::new(MarketRegime::LowActivity));
        let calibrator = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
        let pattern_memory = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));

        let coordinator = EnsembleCoordinator::new(config, regime, calibrator, pattern_memory);

        let candidate = create_test_candidate("mint1", 1000, true);

        let result = coordinator.score_candidate(&candidate, None).await;
        assert!(result.is_ok());

        let (score, confidence, features, reason) = result.unwrap();
        assert!(score <= 100);
        assert!(confidence >= 0.0 && confidence <= 1.0);
        assert!(!features.is_empty());
        assert!(!reason.is_empty());
    }

    #[tokio::test]
    async fn test_record_trade_outcome() {
        let config = EnsembleConfig::default();
        let regime = Arc::new(RwLock::new(MarketRegime::LowActivity));
        let calibrator = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
        let pattern_memory = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));

        let coordinator = EnsembleCoordinator::new(config, regime, calibrator, pattern_memory);

        coordinator
            .record_trade_outcome(
                OracleStrategy::Conservative,
                MarketRegime::LowActivity,
                true,
                0.5,
                0.8,
            )
            .await;

        let stats = coordinator.get_statistics().await;
        assert_eq!(stats.total_strategies, 2);
    }

    #[tokio::test]
    async fn test_ensemble_disabled() {
        let mut config = EnsembleConfig::default();
        config.enabled = false;

        let regime = Arc::new(RwLock::new(MarketRegime::LowActivity));
        let calibrator = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
        let pattern_memory = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));

        let coordinator = EnsembleCoordinator::new(config, regime, calibrator, pattern_memory);

        let candidate = create_test_candidate("mint1", 1000, true);

        let (_, _, _, reason) = coordinator.score_candidate(&candidate, None).await.unwrap();
        assert!(reason.contains("disabled"));
    }

    #[tokio::test]
    async fn test_get_statistics() {
        let config = EnsembleConfig::default();
        let regime = Arc::new(RwLock::new(MarketRegime::Bullish));
        let calibrator = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
        let pattern_memory = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));

        let coordinator = EnsembleCoordinator::new(config, regime, calibrator, pattern_memory);

        let stats = coordinator.get_statistics().await;
        assert_eq!(stats.current_regime, MarketRegime::Bullish);
        assert_eq!(stats.total_strategies, 2);
    }
}
