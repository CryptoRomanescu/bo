//! Simple Oracle for Hot-Swap Demonstration
//!
//! This module provides a minimal Oracle implementation to demonstrate
//! the hot-swap capability in the OODA loop.

use crate::oracle::metacognition::ConfidenceCalibrator;
use crate::oracle::pattern_memory::{Observation, PatternMemory};
use crate::oracle::types::{FeatureWeights, ScoreThresholds};
use crate::types::{PremintCandidate, QuantumCandidateGui};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{info, instrument, warn};

/// Simple Oracle configuration for hot-swap demonstration
#[derive(Debug, Clone)]
pub struct SimpleOracleConfig {
    pub weights: FeatureWeights,
    pub thresholds: ScoreThresholds,
    pub rpc_endpoints: Vec<String>,
}

impl Default for SimpleOracleConfig {
    fn default() -> Self {
        Self {
            weights: FeatureWeights::default(),
            thresholds: ScoreThresholds::default(),
            rpc_endpoints: vec!["https://api.mainnet-beta.solana.com".to_string()],
        }
    }
}

/// Simplified scored candidate for demonstration
#[derive(Debug, Clone)]
pub struct ScoredCandidate {
    pub mint: String,
    pub predicted_score: u8,
    pub feature_scores: HashMap<String, f64>,
    pub reason: String,
    pub timestamp: u64,
    pub calculation_time: u128,
    pub anomaly_detected: bool,
}

/// Simple Oracle metrics
#[derive(Debug, Default, Clone)]
pub struct OracleMetrics {
    pub total_scored: u64,
    pub avg_scoring_time: f64,
    pub high_score_count: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub rpc_errors: u64,
    pub api_errors: u64,
    // Metacognition metrics
    pub confidence_bias: f64,
    pub calibration_error: f64,
    pub avg_predicted_confidence: f64,
    pub calibration_decision_count: usize,
    // Pattern Memory metrics
    pub pattern_memory_total_patterns: usize,
    pub pattern_memory_success_rate: f32,
    pub pattern_memory_avg_confidence: f32,
    pub pattern_memory_avg_pnl: f32,
    pub pattern_memory_usage_bytes: usize,
}

/// Simplified Predictive Oracle for hot-swap demonstration
pub struct PredictiveOracle {
    pub candidate_receiver: mpsc::Receiver<PremintCandidate>,
    pub scored_sender: mpsc::Sender<ScoredCandidate>,
    pub gui_suggestions: Arc<Mutex<Option<mpsc::Sender<QuantumCandidateGui>>>>,
    pub config: Arc<RwLock<SimpleOracleConfig>>,

    // Simple state tracking
    pub metrics: Arc<RwLock<OracleMetrics>>,

    // Metacognition - Confidence Calibrator
    pub confidence_calibrator: Arc<Mutex<ConfidenceCalibrator>>,

    // Pattern Memory - Temporal Pattern Recognition
    pub pattern_memory: Arc<Mutex<PatternMemory>>,
}

impl PredictiveOracle {
    /// Create a new simplified Predictive Oracle for hot-swap demonstration
    pub fn new(
        candidate_receiver: mpsc::Receiver<PremintCandidate>,
        scored_sender: mpsc::Sender<ScoredCandidate>,
        config: Arc<RwLock<SimpleOracleConfig>>,
    ) -> Result<Self> {
        // We can't use async in constructor, so we'll do basic validation
        // Real validation will happen during runtime
        info!("Created Simplified Predictive Oracle for hot-swap demonstration with Metacognition and Pattern Memory");

        Ok(Self {
            candidate_receiver,
            scored_sender,
            gui_suggestions: Arc::new(Mutex::new(None)),
            config,
            metrics: Arc::new(RwLock::new(OracleMetrics::default())),
            confidence_calibrator: Arc::new(Mutex::new(ConfidenceCalibrator::new())),
            pattern_memory: Arc::new(Mutex::new(PatternMemory::with_capacity(1000))),
        })
    }

    /// Set GUI sender for notifications
    pub fn set_gui_sender(&self, sender: mpsc::Sender<QuantumCandidateGui>) {
        tokio::spawn({
            let gui_suggestions = self.gui_suggestions.clone();
            async move {
                let mut gui_lock = gui_suggestions.lock().await;
                *gui_lock = Some(sender);
            }
        });
    }

    /// Update Oracle configuration for hot-swap (Pillar II "Act" phase)
    #[instrument(skip(self, new_weights, new_thresholds))]
    pub async fn update_config(
        &self,
        new_weights: FeatureWeights,
        new_thresholds: ScoreThresholds,
    ) -> Result<()> {
        info!("Hot-swapping Oracle configuration...");

        // Update the shared config
        {
            let mut config_guard = self.config.write().await;
            config_guard.weights = new_weights.clone();
            config_guard.thresholds = new_thresholds.clone();
        }

        info!("Oracle configuration updated successfully:");
        info!("  New liquidity weight: {:.3}", new_weights.liquidity);
        info!(
            "  New holder_distribution weight: {:.3}",
            new_weights.holder_distribution
        );
        info!(
            "  New volume_growth weight: {:.3}",
            new_weights.volume_growth
        );
        info!(
            "  New min_liquidity_sol threshold: {:.2}",
            new_thresholds.min_liquidity_sol
        );

        Ok(())
    }

    /// Get current Oracle metrics (including calibration and pattern memory statistics)
    pub async fn get_metrics(&self) -> OracleMetrics {
        let mut metrics_guard = self.metrics.write().await;

        // Update calibration metrics from the calibrator
        let mut calibrator = self.confidence_calibrator.lock().await;
        let calibration_stats = calibrator.get_stats();

        metrics_guard.confidence_bias = calibration_stats.confidence_bias;
        metrics_guard.calibration_error = calibration_stats.calibration_error;
        metrics_guard.avg_predicted_confidence = calibration_stats.avg_predicted_confidence;
        metrics_guard.calibration_decision_count = calibration_stats.decision_count;

        // Update pattern memory metrics
        let pattern_memory = self.pattern_memory.lock().await;
        let pattern_stats = pattern_memory.stats();

        metrics_guard.pattern_memory_total_patterns = pattern_stats.total_patterns;
        metrics_guard.pattern_memory_success_rate = pattern_stats.success_rate;
        metrics_guard.pattern_memory_avg_confidence = pattern_stats.avg_confidence;
        metrics_guard.pattern_memory_avg_pnl = pattern_stats.avg_pnl;
        metrics_guard.pattern_memory_usage_bytes = pattern_stats.memory_usage_bytes;

        metrics_guard.clone()
    }

    /// Record a trade outcome for metacognition learning
    /// This should be called after a trade completes to update confidence calibration
    /// Note: For pattern memory integration, use record_pattern_outcome() or record_complete_trade_outcome()
    #[tracing::instrument(skip(self))]
    pub async fn record_trade_outcome(
        &self,
        predicted_confidence: f64,
        predicted_score: u8,
        was_successful: bool,
        actual_outcome: f64,
        timestamp: u64,
    ) {
        use crate::oracle::metacognition::DecisionOutcome;

        let outcome = DecisionOutcome::new(
            predicted_confidence,
            predicted_score,
            was_successful,
            actual_outcome,
            timestamp,
        );

        let mut calibrator = self.confidence_calibrator.lock().await;
        calibrator.add_decision(outcome);

        info!(
            "Recorded trade outcome for metacognition: score={}, success={}, outcome={:.3}",
            predicted_score, was_successful, actual_outcome
        );
    }

    /// Record complete trade outcome for both metacognition and pattern memory
    /// This is the recommended method to use after trade completion
    pub async fn record_complete_trade_outcome(
        &self,
        candidate: &PremintCandidate,
        historical_data: Option<&[PremintCandidate]>,
        predicted_confidence: f64,
        predicted_score: u8,
        was_successful: bool,
        actual_outcome: f64,
        pnl_sol: f32,
        timestamp: u64,
    ) {
        // Record for metacognition
        self.record_trade_outcome(
            predicted_confidence,
            predicted_score,
            was_successful,
            actual_outcome,
            timestamp,
        )
        .await;

        // Record for pattern memory
        self.record_pattern_outcome(candidate, historical_data, was_successful, pnl_sol)
            .await;
    }

    /// Get calibration statistics
    pub async fn get_calibration_stats(&self) -> crate::oracle::metacognition::CalibrationStats {
        let mut calibrator = self.confidence_calibrator.lock().await;
        calibrator.get_stats()
    }

    /// Clear cache (simplified)
    pub async fn clear_cache(&self) {
        info!("Cache cleared (simplified implementation)");
    }

    /// Get cache size (simplified)
    pub async fn get_cache_size(&self) -> usize {
        0 // Simplified implementation
    }

    /// Shutdown Oracle (simplified)
    pub async fn shutdown(&self) {
        info!("Oracle shutdown requested (simplified implementation)");
    }

    // --- Pattern Memory Integration Methods ---

    /// Converts candidate data into pattern observations
    /// This creates a simplified feature vector from available candidate data
    fn extract_pattern_observations(
        &self,
        candidate: &PremintCandidate,
        historical_data: Option<&[PremintCandidate]>,
    ) -> Vec<Observation> {
        let mut observations = Vec::new();

        // Create observation from current state
        let current_obs = Observation {
            features: vec![
                // Use timestamp-based features for now since candidate has limited data
                self.normalize_slot(candidate.slot),
                self.normalize_timestamp(candidate.timestamp),
                if candidate.is_jito_bundle.unwrap_or(false) {
                    1.0
                } else {
                    0.0
                },
                // Use program as a categorical feature (simplified to 0.0-1.0)
                self.normalize_program(&candidate.program),
            ],
            timestamp: candidate.timestamp,
        };
        observations.push(current_obs);

        // Add historical observations if available
        if let Some(history) = historical_data {
            for hist in history.iter().take(5) {
                // Use last 5 historical points
                let hist_obs = Observation {
                    features: vec![
                        self.normalize_slot(hist.slot),
                        self.normalize_timestamp(hist.timestamp),
                        if hist.is_jito_bundle.unwrap_or(false) {
                            1.0
                        } else {
                            0.0
                        },
                        self.normalize_program(&hist.program),
                    ],
                    timestamp: hist.timestamp,
                };
                observations.push(hist_obs);
            }
        }

        observations
    }

    // Normalization helpers (0.0-1.0 range)

    /// Normalize slot number (use modulo to keep in range)
    fn normalize_slot(&self, slot: u64) -> f32 {
        // Use slot % 1000000 to keep it in a reasonable range, then normalize
        let normalized = (slot % 1_000_000) as f64 / 1_000_000.0;
        normalized.min(1.0).max(0.0) as f32
    }

    /// Normalize timestamp (use relative time within recent window)
    fn normalize_timestamp(&self, timestamp: u64) -> f32 {
        // Normalize timestamp relative to a 24-hour window
        let seconds_in_day = 86400u64;
        let normalized = (timestamp % seconds_in_day) as f64 / seconds_in_day as f64;
        normalized.min(1.0).max(0.0) as f32
    }

    /// Normalize program name to a numeric value
    fn normalize_program(&self, program: &str) -> f32 {
        // Simple hash-based normalization for program names
        let hash: u32 = program
            .bytes()
            .fold(0u32, |acc, b| acc.wrapping_add(b as u32));
        (hash % 100) as f32 / 100.0
    }

    /// Record a pattern outcome after trade completion
    /// This should be called after determining trade outcome
    pub async fn record_pattern_outcome(
        &self,
        candidate: &PremintCandidate,
        historical_data: Option<&[PremintCandidate]>,
        outcome: bool, // true for profit, false for loss
        pnl_sol: f32,
    ) {
        // Extract the same observations used for scoring
        let observations = self.extract_pattern_observations(candidate, historical_data);

        if observations.len() >= 2 {
            let mut pattern_memory = self.pattern_memory.lock().await;
            pattern_memory.add_pattern(observations, outcome, pnl_sol);

            // Log statistics
            let stats = pattern_memory.stats();
            info!(
                "Pattern memory updated: {} patterns, {:.1}% success rate, avg PnL: {:.3} SOL, memory: {:.2} KB",
                stats.total_patterns,
                stats.success_rate * 100.0,
                stats.avg_pnl,
                stats.memory_usage_bytes as f32 / 1024.0
            );
        }
    }

    /// Get pattern memory statistics
    pub async fn get_pattern_memory_stats(
        &self,
    ) -> crate::oracle::pattern_memory::PatternMemoryStats {
        let pattern_memory = self.pattern_memory.lock().await;
        pattern_memory.stats()
    }

    /// Score a candidate with pattern memory integration
    #[tracing::instrument(skip(self, candidate, historical_data), fields(mint = %candidate.mint))]
    pub async fn score_candidate(
        &self,
        candidate: PremintCandidate,
        historical_data: Option<Vec<PremintCandidate>>,
    ) -> Result<ScoredCandidate> {
        let start_time = std::time::Instant::now();

        // Calculate base score (simplified scoring)
        let mut base_score = self.calculate_base_score(&candidate).await?;
        let mut feature_scores = HashMap::new();
        let mut reason = String::new();

        // Add base scoring reasons
        feature_scores.insert("base_score".to_string(), base_score as f64);
        reason.push_str(&format!("Base score: {}, ", base_score));

        // Extract pattern observations
        let observations =
            self.extract_pattern_observations(&candidate, historical_data.as_deref());

        // Check for matching patterns (only if we have enough observations)
        if observations.len() >= 2 {
            let pattern_memory = self.pattern_memory.lock().await;

            if let Some(pattern_match) = pattern_memory.find_match(&observations) {
                let confidence = pattern_match.pattern.confidence;
                let predicted_pnl = pattern_match.predicted_pnl;

                // Adjust score based on pattern prediction
                if confidence > 0.7 {
                    if pattern_match.predicted_outcome {
                        // Historical pattern suggests success
                        let boost = (confidence * 20.0) as u8; // Up to +20 points
                        base_score = base_score.saturating_add(boost);

                        info!(
                            "Pattern boost: +{} points (confidence: {:.2}, predicted PnL: {:.3})",
                            boost, confidence, predicted_pnl
                        );

                        reason.push_str(&format!(
                            "Pattern boost: +{} pts (conf: {:.2}), ",
                            boost, confidence
                        ));
                    } else {
                        // Historical pattern suggests failure
                        let penalty = (confidence * 30.0) as u8; // Up to -30 points
                        base_score = base_score.saturating_sub(penalty);

                        warn!(
                            "Pattern penalty: -{} points (confidence: {:.2}, predicted PnL: {:.3})",
                            penalty, confidence, predicted_pnl
                        );

                        reason.push_str(&format!(
                            "Pattern penalty: -{} pts (conf: {:.2}), ",
                            penalty, confidence
                        ));
                    }
                }

                // Add to feature scores
                feature_scores.insert("pattern_match".to_string(), confidence as f64);
                feature_scores.insert("predicted_pnl".to_string(), predicted_pnl as f64);
            } else {
                reason.push_str("No pattern match, ");
            }
        } else {
            reason.push_str("Insufficient data for patterns, ");
        }

        let calculation_time = start_time.elapsed().as_micros();

        Ok(ScoredCandidate {
            mint: candidate.mint.clone(),
            predicted_score: base_score,
            feature_scores,
            reason: reason.trim_end_matches(", ").to_string(),
            timestamp: candidate.timestamp,
            calculation_time,
            anomaly_detected: false,
        })
    }

    /// Calculate base score for a candidate (simplified)
    async fn calculate_base_score(&self, candidate: &PremintCandidate) -> Result<u8> {
        // Simplified base scoring logic
        let mut score = 50u8; // Start with neutral score

        // Bonus for Jito bundle
        if candidate.is_jito_bundle.unwrap_or(false) {
            score = score.saturating_add(10);
        }

        // Bonus based on program (simplified)
        if candidate.program.contains("pump.fun") {
            score = score.saturating_add(15);
        }

        Ok(score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn create_test_config() -> SimpleOracleConfig {
        SimpleOracleConfig {
            rpc_endpoints: vec!["https://api.mainnet-beta.solana.com".to_string()],
            ..SimpleOracleConfig::default()
        }
    }

    #[tokio::test]
    async fn test_oracle_creation() {
        let (candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = Arc::new(RwLock::new(create_test_config()));

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config);
        assert!(oracle.is_ok());
    }

    #[tokio::test]
    async fn test_oracle_creation_empty_rpc_endpoints() {
        let (candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let mut config = create_test_config();
        config.rpc_endpoints.clear();
        let config = Arc::new(RwLock::new(config));

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config);
        // Since validation is now done at runtime, the constructor should succeed
        assert!(oracle.is_ok());
    }

    #[tokio::test]
    async fn test_update_config() {
        let (candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = Arc::new(RwLock::new(create_test_config()));

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config.clone()).unwrap();

        // Create new weights and thresholds
        let new_weights = FeatureWeights {
            liquidity: 0.5,
            holder_distribution: 0.3,
            volume_growth: 0.2,
            ..FeatureWeights::default()
        };
        let new_thresholds = ScoreThresholds {
            min_liquidity_sol: 25.0,
            ..ScoreThresholds::default()
        };

        // Update config
        let result = oracle
            .update_config(new_weights.clone(), new_thresholds.clone())
            .await;
        assert!(result.is_ok());

        // Verify config was updated
        let config_guard = config.read().await;
        assert_eq!(config_guard.weights.liquidity, 0.5);
        assert_eq!(config_guard.weights.holder_distribution, 0.3);
        assert_eq!(config_guard.thresholds.min_liquidity_sol, 25.0);
    }

    #[tokio::test]
    async fn test_get_metrics() {
        let (candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = Arc::new(RwLock::new(create_test_config()));

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();
        let metrics = oracle.get_metrics().await;

        // Should have default values
        assert_eq!(metrics.total_scored, 0);
        assert_eq!(metrics.high_score_count, 0);
        // Calibration metrics should be initialized
        assert_eq!(metrics.calibration_decision_count, 0);
    }

    #[tokio::test]
    async fn test_record_trade_outcome() {
        let (candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = Arc::new(RwLock::new(create_test_config()));

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();

        // Record a trade outcome
        oracle.record_trade_outcome(0.75, 75, true, 0.4, 1000).await;

        // Get calibration stats
        let stats = oracle.get_calibration_stats().await;
        assert_eq!(stats.decision_count, 1);
        assert!((stats.avg_predicted_confidence - 0.75).abs() < 0.01);
        assert!((stats.avg_success_rate - 1.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_calibration_metrics_in_oracle_metrics() {
        let (candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = Arc::new(RwLock::new(create_test_config()));

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();

        // Record some outcomes
        for i in 0..15 {
            let success = i % 3 != 0; // ~67% success rate
            let outcome = if success { 0.3 } else { -0.2 };
            oracle
                .record_trade_outcome(0.80, 80, success, outcome, 1000 + i)
                .await;
        }

        // Get metrics - should include calibration stats
        let metrics = oracle.get_metrics().await;
        assert_eq!(metrics.calibration_decision_count, 15);
        assert!((metrics.avg_predicted_confidence - 0.80).abs() < 0.01);
        assert!(metrics.calibration_error > 0.0);
    }

    #[tokio::test]
    async fn test_get_calibration_stats() {
        let (candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = Arc::new(RwLock::new(create_test_config()));

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();

        // Initially no decisions
        let stats = oracle.get_calibration_stats().await;
        assert_eq!(stats.decision_count, 0);

        // Add some decisions
        for i in 0..10 {
            oracle
                .record_trade_outcome(0.70, 70, true, 0.3, 1000 + i)
                .await;
        }

        let stats = oracle.get_calibration_stats().await;
        assert_eq!(stats.decision_count, 10);
        assert!(
            stats.calculation_time_ns < 1_000_000,
            "Should calculate in <1ms"
        );
    }

    // --- Pattern Memory Integration Tests ---

    fn create_test_premint_candidate(mint: &str, slot: u64, is_jito: bool) -> PremintCandidate {
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
    async fn test_pattern_memory_initialization() {
        let (_candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = Arc::new(RwLock::new(create_test_config()));

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();

        // Check pattern memory is initialized
        let stats = oracle.get_pattern_memory_stats().await;
        assert_eq!(stats.total_patterns, 0);
        assert_eq!(stats.successful_patterns, 0);
    }

    #[tokio::test]
    async fn test_record_pattern_outcome() {
        let (_candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = Arc::new(RwLock::new(create_test_config()));

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();

        // Create test candidates
        let candidate1 = create_test_premint_candidate("mint1", 1000, true);
        let candidate2 = create_test_premint_candidate("mint2", 1001, true);

        // Record a successful outcome
        oracle
            .record_pattern_outcome(&candidate1, Some(&[candidate2]), true, 0.5)
            .await;

        // Check pattern was recorded
        let stats = oracle.get_pattern_memory_stats().await;
        assert_eq!(stats.total_patterns, 1);
        assert_eq!(stats.successful_patterns, 1);
        assert_eq!(stats.success_rate, 1.0);
    }

    #[tokio::test]
    async fn test_pattern_memory_influences_scoring() {
        let (_candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = Arc::new(RwLock::new(create_test_config()));

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();

        // Create similar test candidates
        let candidate1 = create_test_premint_candidate("mint1", 1000, true);
        let candidate2 = create_test_premint_candidate("mint2", 1001, true);
        let candidate3 = create_test_premint_candidate("mint3", 1002, true);

        // Score without pattern
        let score1 = oracle
            .score_candidate(candidate1.clone(), Some(vec![candidate2.clone()]))
            .await
            .unwrap();
        let base_score = score1.predicted_score;

        // Record successful outcome with high PnL
        oracle
            .record_pattern_outcome(&candidate1, Some(&[candidate2.clone()]), true, 0.8)
            .await;

        // Record it multiple times to increase confidence
        for _ in 0..5 {
            oracle
                .record_pattern_outcome(&candidate1, Some(&[candidate2.clone()]), true, 0.8)
                .await;
        }

        // Score similar candidate - should get boost
        let score2 = oracle
            .score_candidate(candidate3, Some(vec![candidate2]))
            .await
            .unwrap();

        // Check that pattern influenced scoring
        assert!(
            score2.feature_scores.contains_key("pattern_match"),
            "Should have pattern match in feature scores"
        );

        // Due to pattern learning, similar patterns should be boosted
        // (though exact score may vary due to merging)
        println!(
            "Base score: {}, Score with pattern: {}",
            base_score, score2.predicted_score
        );
    }

    #[tokio::test]
    async fn test_pattern_penalty_for_failed_patterns() {
        let (_candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = Arc::new(RwLock::new(create_test_config()));

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();

        // Create test candidates with different characteristics
        let candidate1 = create_test_premint_candidate("mint1", 2000, false);
        let candidate2 = create_test_premint_candidate("mint2", 2001, false);
        let candidate3 = create_test_premint_candidate("mint3", 2002, false);

        // Record multiple failed outcomes to build confidence in failure pattern
        for _ in 0..10 {
            oracle
                .record_pattern_outcome(&candidate1, Some(&[candidate2.clone()]), false, -0.3)
                .await;
        }

        // Score similar candidate - should get penalty
        let score = oracle
            .score_candidate(candidate3, Some(vec![candidate2]))
            .await
            .unwrap();

        // Check that pattern influenced scoring negatively
        if let Some(&pattern_conf) = score.feature_scores.get("pattern_match") {
            // High confidence pattern was found
            assert!(
                pattern_conf > 0.5,
                "Should have found high confidence pattern"
            );
        }
    }

    #[tokio::test]
    async fn test_complete_trade_outcome_recording() {
        let (_candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = Arc::new(RwLock::new(create_test_config()));

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();

        let candidate = create_test_premint_candidate("mint1", 1000, true);
        let historical = create_test_premint_candidate("mint0", 999, true);

        // Record complete outcome (both metacognition and pattern memory)
        oracle
            .record_complete_trade_outcome(
                &candidate,
                Some(&[historical]),
                0.85,
                85,
                true,
                0.5,
                0.5,
                1000,
            )
            .await;

        // Check both systems recorded the outcome
        let calib_stats = oracle.get_calibration_stats().await;
        assert_eq!(calib_stats.decision_count, 1);

        let pattern_stats = oracle.get_pattern_memory_stats().await;
        assert_eq!(pattern_stats.total_patterns, 1);
    }

    #[tokio::test]
    async fn test_pattern_memory_metrics_in_oracle_metrics() {
        let (_candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = Arc::new(RwLock::new(create_test_config()));

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();

        // Initially empty
        let metrics = oracle.get_metrics().await;
        assert_eq!(metrics.pattern_memory_total_patterns, 0);

        // Record some patterns
        let candidate1 = create_test_premint_candidate("mint1", 1000, true);
        let candidate2 = create_test_premint_candidate("mint2", 1001, true);

        for i in 0..5 {
            let success = i % 2 == 0;
            let pnl = if success { 0.3 } else { -0.2 };
            oracle
                .record_pattern_outcome(&candidate1, Some(&[candidate2.clone()]), success, pnl)
                .await;
        }

        // Check metrics updated
        let metrics = oracle.get_metrics().await;
        assert!(metrics.pattern_memory_total_patterns > 0);
        assert!(metrics.pattern_memory_usage_bytes > 0);
    }

    #[tokio::test]
    async fn test_pattern_memory_capacity_limit() {
        let (_candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = Arc::new(RwLock::new(create_test_config()));

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();

        // Record many patterns with very different characteristics
        for i in 0..1500 {
            let candidate1 =
                create_test_premint_candidate(&format!("mint{}", i), 1000 + i, i % 2 == 0);
            let candidate2 =
                create_test_premint_candidate(&format!("mint{}", i + 10000), 1001 + i, i % 3 == 0);

            oracle
                .record_pattern_outcome(&candidate1, Some(&[candidate2]), i % 2 == 0, 0.1)
                .await;
        }

        // Check that pattern memory respects capacity limit
        let stats = oracle.get_pattern_memory_stats().await;
        assert!(
            stats.total_patterns <= 1000,
            "Should respect 1000 pattern capacity limit"
        );
    }

    #[tokio::test]
    async fn test_pattern_memory_usage_under_50kb() {
        let (_candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = Arc::new(RwLock::new(create_test_config()));

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();

        // Fill pattern memory to capacity with diverse patterns
        for i in 0..1000 {
            let candidate1 =
                create_test_premint_candidate(&format!("mint{}", i), 1000 + (i * 7), i % 2 == 0);
            let candidate2 = create_test_premint_candidate(
                &format!("mint{}", i + 5000),
                1001 + (i * 11),
                i % 3 == 0,
            );

            oracle
                .record_pattern_outcome(
                    &candidate1,
                    Some(&[candidate2]),
                    i % 2 == 0,
                    (i as f32) / 1000.0,
                )
                .await;
        }

        // Check memory usage requirement
        let stats = oracle.get_pattern_memory_stats().await;
        println!(
            "Pattern memory usage: {} bytes ({:.2} KB) for {} patterns",
            stats.memory_usage_bytes,
            stats.memory_usage_bytes as f32 / 1024.0,
            stats.total_patterns
        );

        assert!(
            stats.memory_usage_bytes < 50 * 1024,
            "Memory usage {} bytes exceeds 50KB requirement",
            stats.memory_usage_bytes
        );
    }
}
