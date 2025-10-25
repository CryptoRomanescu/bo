//! QuantumVoter - Bayesian voting mechanism for ensemble oracle decisions

use anyhow::Result;
use std::collections::HashMap;
use tracing::{debug, info};

use crate::oracle::ensemble::types::{
    EnsembleConfig, OracleStrategy, StrategyPerformance, VotingResult,
};

/// QuantumVoter implements Bayesian voting to aggregate predictions
pub struct QuantumVoter {
    config: EnsembleConfig,
}

impl QuantumVoter {
    /// Create a new QuantumVoter
    pub fn new(config: EnsembleConfig) -> Self {
        Self { config }
    }

    /// Aggregate predictions from multiple oracles using Bayesian voting
    ///
    /// # Arguments
    /// * `predictions` - Map of strategy to (score, confidence, feature_scores)
    /// * `performance_metrics` - Historical performance per strategy
    ///
    /// # Returns
    /// VotingResult with aggregated score and confidence
    pub fn vote(
        &self,
        predictions: HashMap<OracleStrategy, (u8, f64, HashMap<String, f64>)>,
        performance_metrics: &HashMap<OracleStrategy, StrategyPerformance>,
    ) -> Result<VotingResult> {
        if predictions.is_empty() {
            anyhow::bail!("Cannot vote with empty predictions");
        }

        debug!("QuantumVoter aggregating {} predictions", predictions.len());

        // Calculate weights for each strategy based on performance
        let weights = self.calculate_bayesian_weights(&predictions, performance_metrics);

        // Aggregate scores using weighted voting
        let (final_score, final_confidence, uncertainty) =
            self.aggregate_predictions(&predictions, &weights);

        // Build strategy votes map
        let mut strategy_votes = HashMap::new();
        for (strategy, (score, confidence, _)) in &predictions {
            strategy_votes.insert(*strategy, (*score, *confidence));
        }

        // Build reason string
        let reason = self.build_reason_string(&predictions, &weights, final_score);

        info!(
            "QuantumVoter result: score={}, confidence={:.3}, uncertainty={:.3}",
            final_score, final_confidence, uncertainty
        );

        Ok(VotingResult {
            final_score,
            final_confidence,
            strategy_votes,
            strategy_weights: weights,
            uncertainty,
            reason,
        })
    }

    /// Calculate Bayesian weights for each strategy
    ///
    /// Weight = (Performance Prior) * (Prediction Likelihood)
    ///
    /// Performance Prior: Based on historical win rate and profit factor
    /// Prediction Likelihood: Based on prediction confidence
    fn calculate_bayesian_weights(
        &self,
        predictions: &HashMap<OracleStrategy, (u8, f64, HashMap<String, f64>)>,
        performance_metrics: &HashMap<OracleStrategy, StrategyPerformance>,
    ) -> HashMap<OracleStrategy, f64> {
        let mut weights = HashMap::new();
        let prior_strength = self.config.bayesian_prior_strength;

        for (strategy, (score, confidence, _)) in predictions {
            // Calculate performance prior
            let prior = if let Some(perf) = performance_metrics.get(strategy) {
                if perf.trade_count >= 10 {
                    // Sufficient data: use historical performance
                    let perf_score = perf.win_rate * perf.profit_factor.min(3.0);
                    prior_strength * perf_score + (1.0 - prior_strength) * 0.5
                } else if perf.trade_count > 0 {
                    // Some data: blend with neutral prior
                    let perf_score = perf.win_rate * perf.profit_factor.min(3.0);
                    let data_weight = perf.trade_count as f64 / 10.0;
                    data_weight * perf_score + (1.0 - data_weight) * 0.5
                } else {
                    // No data: neutral prior
                    0.5
                }
            } else {
                // No performance data: neutral prior
                0.5
            };

            // Calculate likelihood based on prediction confidence and score
            let score_normalized = (*score as f64) / 100.0;
            let likelihood = confidence * score_normalized;

            // Bayesian weight: prior * likelihood
            let weight = prior * likelihood;

            debug!(
                "Strategy {} weight calculation: prior={:.3}, likelihood={:.3}, weight={:.3}",
                strategy, prior, likelihood, weight
            );

            weights.insert(*strategy, weight);
        }

        // Normalize weights to sum to 1.0
        let total_weight: f64 = weights.values().sum();
        if total_weight > 0.0 {
            for weight in weights.values_mut() {
                *weight /= total_weight;
            }
        } else {
            // Fallback: equal weights
            let equal_weight = 1.0 / predictions.len() as f64;
            for weight in weights.values_mut() {
                *weight = equal_weight;
            }
        }

        weights
    }

    /// Aggregate predictions using weighted voting
    ///
    /// Returns (final_score, final_confidence, uncertainty)
    fn aggregate_predictions(
        &self,
        predictions: &HashMap<OracleStrategy, (u8, f64, HashMap<String, f64>)>,
        weights: &HashMap<OracleStrategy, f64>,
    ) -> (u8, f64, f64) {
        let mut weighted_score = 0.0;
        let mut weighted_confidence = 0.0;
        let mut score_variance = 0.0;

        // Calculate weighted average score and confidence
        for (strategy, (score, confidence, _)) in predictions {
            let weight = weights.get(strategy).unwrap_or(&0.0);
            weighted_score += (*score as f64) * weight;
            weighted_confidence += confidence * weight;
        }

        // Calculate uncertainty (variance in scores)
        for (strategy, (score, _, _)) in predictions {
            let weight = weights.get(strategy).unwrap_or(&0.0);
            let diff = (*score as f64) - weighted_score;
            score_variance += weight * diff * diff;
        }

        let uncertainty = score_variance.sqrt() / 100.0; // Normalize to 0-1

        // Round final score
        let final_score = weighted_score.round().min(100.0).max(0.0) as u8;

        // Adjust confidence based on uncertainty
        let uncertainty_penalty = 1.0 - (uncertainty * 0.5); // Max 50% penalty
        let final_confidence = (weighted_confidence * uncertainty_penalty)
            .min(1.0)
            .max(0.0);

        (final_score, final_confidence, uncertainty)
    }

    /// Build explanation string for the voting result
    fn build_reason_string(
        &self,
        predictions: &HashMap<OracleStrategy, (u8, f64, HashMap<String, f64>)>,
        weights: &HashMap<OracleStrategy, f64>,
        final_score: u8,
    ) -> String {
        let mut parts = vec![format!("Ensemble vote: score={}", final_score)];

        // Sort strategies by weight for consistent ordering
        let mut strategy_weights: Vec<_> = weights.iter().collect();
        strategy_weights.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());

        for (strategy, weight) in strategy_weights.iter().take(3) {
            if let Some((score, conf, _)) = predictions.get(strategy) {
                parts.push(format!(
                    "{}(s={}, c={:.2}, w={:.2})",
                    strategy, score, conf, weight
                ));
            }
        }

        parts.join(", ")
    }

    /// Calculate consensus level (how much strategies agree)
    ///
    /// Returns 0.0 (no consensus) to 1.0 (perfect consensus)
    pub fn calculate_consensus(
        &self,
        predictions: &HashMap<OracleStrategy, (u8, f64, HashMap<String, f64>)>,
    ) -> f64 {
        if predictions.len() < 2 {
            return 1.0; // Single prediction = perfect consensus
        }

        let scores: Vec<u8> = predictions.values().map(|(s, _, _)| *s).collect();

        // Calculate coefficient of variation
        let mean = scores.iter().map(|&s| s as f64).sum::<f64>() / scores.len() as f64;
        if mean < 0.01 {
            return 0.0; // All scores near zero
        }

        let variance = scores
            .iter()
            .map(|&s| {
                let diff = s as f64 - mean;
                diff * diff
            })
            .sum::<f64>()
            / scores.len() as f64;

        let std_dev = variance.sqrt();
        let cv = std_dev / mean; // Coefficient of variation

        // Map CV to consensus: lower CV = higher consensus
        // CV of 0 = consensus of 1.0
        // CV of 0.5 or higher = consensus of 0
        (1.0 - (cv * 2.0)).max(0.0).min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::types::MarketRegime;

    fn create_test_voter() -> QuantumVoter {
        QuantumVoter::new(EnsembleConfig::default())
    }

    fn create_test_performance(
        strategy: OracleStrategy,
        win_rate: f64,
        pf: f64,
        count: usize,
    ) -> StrategyPerformance {
        StrategyPerformance {
            strategy,
            win_rate,
            profit_factor: pf,
            avg_confidence: 0.7,
            trade_count: count,
            regime: MarketRegime::LowActivity,
            last_updated: 1000,
        }
    }

    #[test]
    fn test_voter_creation() {
        let voter = create_test_voter();
        assert!(voter.config.enabled);
    }

    #[test]
    fn test_voting_with_single_prediction() {
        let voter = create_test_voter();

        let mut predictions = HashMap::new();
        predictions.insert(OracleStrategy::Conservative, (75u8, 0.8, HashMap::new()));

        let performance = HashMap::new();

        let result = voter.vote(predictions, &performance).unwrap();

        assert_eq!(result.final_score, 75);
        assert!(result.final_confidence > 0.0);
        assert_eq!(result.strategy_votes.len(), 1);
    }

    #[test]
    fn test_voting_with_multiple_predictions() {
        let voter = create_test_voter();

        let mut predictions = HashMap::new();
        predictions.insert(OracleStrategy::Conservative, (70u8, 0.8, HashMap::new()));
        predictions.insert(OracleStrategy::Aggressive, (80u8, 0.7, HashMap::new()));

        let performance = HashMap::new();

        let result = voter.vote(predictions, &performance).unwrap();

        // Final score should be between 70 and 80
        assert!(result.final_score >= 70 && result.final_score <= 80);
        assert_eq!(result.strategy_votes.len(), 2);
    }

    #[test]
    fn test_bayesian_weights_with_performance() {
        let voter = create_test_voter();

        let mut predictions = HashMap::new();
        predictions.insert(OracleStrategy::Conservative, (70u8, 0.8, HashMap::new()));
        predictions.insert(OracleStrategy::Aggressive, (80u8, 0.7, HashMap::new()));

        let mut performance = HashMap::new();
        performance.insert(
            OracleStrategy::Conservative,
            create_test_performance(OracleStrategy::Conservative, 0.8, 2.0, 20),
        );
        performance.insert(
            OracleStrategy::Aggressive,
            create_test_performance(OracleStrategy::Aggressive, 0.5, 1.2, 20),
        );

        let result = voter.vote(predictions, &performance).unwrap();

        // Conservative should have higher weight due to better performance
        let conservative_weight = result
            .strategy_weights
            .get(&OracleStrategy::Conservative)
            .unwrap();
        let aggressive_weight = result
            .strategy_weights
            .get(&OracleStrategy::Aggressive)
            .unwrap();

        assert!(conservative_weight > aggressive_weight);
    }

    #[test]
    fn test_consensus_calculation_perfect() {
        let voter = create_test_voter();

        let mut predictions = HashMap::new();
        predictions.insert(OracleStrategy::Conservative, (75u8, 0.8, HashMap::new()));
        predictions.insert(OracleStrategy::Aggressive, (75u8, 0.8, HashMap::new()));

        let consensus = voter.calculate_consensus(&predictions);

        // Perfect agreement
        assert!(consensus > 0.95);
    }

    #[test]
    fn test_consensus_calculation_disagreement() {
        let voter = create_test_voter();

        let mut predictions = HashMap::new();
        predictions.insert(OracleStrategy::Conservative, (30u8, 0.8, HashMap::new()));
        predictions.insert(OracleStrategy::Aggressive, (90u8, 0.8, HashMap::new()));

        let consensus = voter.calculate_consensus(&predictions);

        // Strong disagreement
        assert!(consensus < 0.5);
    }

    #[test]
    fn test_uncertainty_calculation() {
        let voter = create_test_voter();

        let mut predictions = HashMap::new();
        predictions.insert(OracleStrategy::Conservative, (50u8, 0.8, HashMap::new()));
        predictions.insert(OracleStrategy::Aggressive, (90u8, 0.8, HashMap::new()));

        let performance = HashMap::new();
        let result = voter.vote(predictions, &performance).unwrap();

        // High disagreement should produce measurable uncertainty
        // Uncertainty is normalized (std_dev / 100), so expect > 0.1 for 40 point difference
        assert!(
            result.uncertainty > 0.1,
            "Expected uncertainty > 0.1, got {}",
            result.uncertainty
        );

        // High uncertainty should reduce confidence somewhat
        assert!(
            result.final_confidence < 0.85,
            "Expected confidence < 0.85, got {}",
            result.final_confidence
        );
    }

    #[test]
    fn test_weight_normalization() {
        let voter = create_test_voter();

        let mut predictions = HashMap::new();
        predictions.insert(OracleStrategy::Conservative, (70u8, 0.8, HashMap::new()));
        predictions.insert(OracleStrategy::Aggressive, (80u8, 0.7, HashMap::new()));

        let performance = HashMap::new();
        let result = voter.vote(predictions, &performance).unwrap();

        // Weights should sum to 1.0
        let total_weight: f64 = result.strategy_weights.values().sum();
        assert!((total_weight - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_voting_with_zero_scores() {
        let voter = create_test_voter();

        let mut predictions = HashMap::new();
        predictions.insert(OracleStrategy::Conservative, (0u8, 0.5, HashMap::new()));
        predictions.insert(OracleStrategy::Aggressive, (0u8, 0.5, HashMap::new()));

        let performance = HashMap::new();
        let result = voter.vote(predictions, &performance).unwrap();

        // Should handle zero scores gracefully
        assert_eq!(result.final_score, 0);
    }

    #[test]
    fn test_empty_predictions_error() {
        let voter = create_test_voter();
        let predictions = HashMap::new();
        let performance = HashMap::new();

        let result = voter.vote(predictions, &performance);
        assert!(result.is_err());
    }
}
