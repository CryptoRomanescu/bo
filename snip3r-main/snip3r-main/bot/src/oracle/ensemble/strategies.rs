//! Oracle strategy implementations for the ensemble system

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info};

use crate::oracle::ensemble::types::StrategyConfig;
use crate::oracle::metacognition::ConfidenceCalibrator;
use crate::oracle::pattern_memory::{Observation, PatternMemory};
use crate::types::PremintCandidate;

/// Trait for oracle strategies
#[async_trait::async_trait]
pub trait OracleStrategyTrait: Send + Sync {
    /// Score a candidate with this strategy
    async fn score_candidate(
        &self,
        candidate: &PremintCandidate,
        historical_data: Option<&[PremintCandidate]>,
    ) -> Result<(u8, f64, HashMap<String, f64>)>; // (score, confidence, feature_scores)

    /// Get strategy configuration
    fn get_config(&self) -> &StrategyConfig;
}

/// Conservative oracle strategy - risk-averse approach
pub struct ConservativeOracle {
    config: StrategyConfig,
    confidence_calibrator: Arc<Mutex<ConfidenceCalibrator>>,
    pattern_memory: Arc<Mutex<PatternMemory>>,
}

impl ConservativeOracle {
    /// Create a new conservative oracle
    pub fn new(
        confidence_calibrator: Arc<Mutex<ConfidenceCalibrator>>,
        pattern_memory: Arc<Mutex<PatternMemory>>,
    ) -> Self {
        Self {
            config: StrategyConfig::conservative(),
            confidence_calibrator,
            pattern_memory,
        }
    }

    /// Create with custom configuration
    pub fn with_config(
        config: StrategyConfig,
        confidence_calibrator: Arc<Mutex<ConfidenceCalibrator>>,
        pattern_memory: Arc<Mutex<PatternMemory>>,
    ) -> Self {
        Self {
            config,
            confidence_calibrator,
            pattern_memory,
        }
    }

    /// Calculate base score using conservative approach
    fn calculate_base_score(&self, candidate: &PremintCandidate) -> (u8, HashMap<String, f64>) {
        let mut score = 0u8;
        let mut feature_scores = HashMap::new();

        // Conservative scoring emphasizes safety and quality

        // Jito bundle - moderate bonus (conservative on MEV reliance)
        if candidate.is_jito_bundle.unwrap_or(false) {
            score = score.saturating_add(8);
            feature_scores.insert("jito_bundle".to_string(), 8.0);
        }

        // Program bonus - conservative, only for well-known programs
        if candidate.program.contains("pump.fun") {
            score = score.saturating_add(12);
            feature_scores.insert("program_quality".to_string(), 12.0);
        } else if candidate.program.contains("raydium") || candidate.program.contains("orca") {
            score = score.saturating_add(10);
            feature_scores.insert("program_quality".to_string(), 10.0);
        } else {
            // Unknown programs get penalty in conservative mode
            feature_scores.insert("program_quality".to_string(), 0.0);
        }

        // Instruction quality - conservative prefers simple operations
        if let Some(ref instr) = candidate.instruction_summary {
            if instr.contains("create") || instr.contains("initialize") {
                score = score.saturating_add(5);
                feature_scores.insert("instruction_quality".to_string(), 5.0);
            }
        }

        // Start with higher base for conservative (requires more to pass threshold)
        score = score.saturating_add(30);
        feature_scores.insert("base_score".to_string(), 30.0);

        (score, feature_scores)
    }

    /// Extract pattern observations from candidate data
    fn extract_observations(
        &self,
        candidate: &PremintCandidate,
        historical_data: Option<&[PremintCandidate]>,
    ) -> Vec<Observation> {
        let mut observations = Vec::new();

        let current_obs = Observation {
            features: vec![
                Self::normalize_slot(candidate.slot),
                Self::normalize_timestamp(candidate.timestamp),
                if candidate.is_jito_bundle.unwrap_or(false) {
                    1.0
                } else {
                    0.0
                },
                Self::normalize_program(&candidate.program),
            ],
            timestamp: candidate.timestamp,
        };
        observations.push(current_obs);

        if let Some(history) = historical_data {
            for hist in history.iter().take(5) {
                let hist_obs = Observation {
                    features: vec![
                        Self::normalize_slot(hist.slot),
                        Self::normalize_timestamp(hist.timestamp),
                        if hist.is_jito_bundle.unwrap_or(false) {
                            1.0
                        } else {
                            0.0
                        },
                        Self::normalize_program(&hist.program),
                    ],
                    timestamp: hist.timestamp,
                };
                observations.push(hist_obs);
            }
        }

        observations
    }

    fn normalize_slot(slot: u64) -> f32 {
        let normalized = (slot % 1_000_000) as f64 / 1_000_000.0;
        normalized.min(1.0).max(0.0) as f32
    }

    fn normalize_timestamp(timestamp: u64) -> f32 {
        let seconds_in_day = 86400u64;
        let normalized = (timestamp % seconds_in_day) as f64 / seconds_in_day as f64;
        normalized.min(1.0).max(0.0) as f32
    }

    fn normalize_program(program: &str) -> f32 {
        let hash: u32 = program
            .bytes()
            .fold(0u32, |acc, b| acc.wrapping_add(b as u32));
        (hash % 100) as f32 / 100.0
    }
}

#[async_trait::async_trait]
impl OracleStrategyTrait for ConservativeOracle {
    async fn score_candidate(
        &self,
        candidate: &PremintCandidate,
        historical_data: Option<&[PremintCandidate]>,
    ) -> Result<(u8, f64, HashMap<String, f64>)> {
        debug!("ConservativeOracle scoring candidate: {}", candidate.mint);

        // Calculate base score
        let (mut score, mut feature_scores) = self.calculate_base_score(candidate);

        // Check pattern memory with conservative adjustments
        let observations = self.extract_observations(candidate, historical_data);

        if observations.len() >= 2 {
            let pattern_memory = self.pattern_memory.lock().await;

            if let Some(pattern_match) = pattern_memory.find_match(&observations) {
                let confidence = pattern_match.pattern.confidence;

                // Conservative: only trust high-confidence patterns
                if confidence > 0.8 {
                    if pattern_match.predicted_outcome {
                        let boost =
                            (confidence as f64 * 20.0 * self.config.pattern_boost_multiplier) as u8;
                        score = score.saturating_add(boost);
                        feature_scores.insert("pattern_boost".to_string(), boost as f64);
                        info!(
                            "Conservative pattern boost: +{} pts (conf: {:.2})",
                            boost, confidence
                        );
                    } else {
                        // Conservative: heavily penalize bad patterns
                        let penalty =
                            (confidence as f64 * 30.0 * self.config.pattern_penalty_multiplier)
                                as u8;
                        score = score.saturating_sub(penalty);
                        feature_scores.insert("pattern_penalty".to_string(), -(penalty as f64));
                        info!(
                            "Conservative pattern penalty: -{} pts (conf: {:.2})",
                            penalty, confidence
                        );
                    }
                    feature_scores.insert("pattern_confidence".to_string(), confidence as f64);
                }
            }
        }

        // Apply minimum threshold
        if score < self.config.min_score_threshold {
            score = 0; // Reject if below threshold
        }

        // Calculate confidence (conservative: lower confidence in general)
        let base_confidence = (score as f64 / 100.0) * 0.8; // Conservative multiplier
        let mut calibrator = self.confidence_calibrator.lock().await;
        let calibrated_confidence = calibrator.calibrate_confidence(base_confidence);

        Ok((score, calibrated_confidence, feature_scores))
    }

    fn get_config(&self) -> &StrategyConfig {
        &self.config
    }
}

/// Aggressive oracle strategy - opportunity-seeking approach
pub struct AggressiveOracle {
    config: StrategyConfig,
    confidence_calibrator: Arc<Mutex<ConfidenceCalibrator>>,
    pattern_memory: Arc<Mutex<PatternMemory>>,
}

impl AggressiveOracle {
    /// Create a new aggressive oracle
    pub fn new(
        confidence_calibrator: Arc<Mutex<ConfidenceCalibrator>>,
        pattern_memory: Arc<Mutex<PatternMemory>>,
    ) -> Self {
        Self {
            config: StrategyConfig::aggressive(),
            confidence_calibrator,
            pattern_memory,
        }
    }

    /// Create with custom configuration
    pub fn with_config(
        config: StrategyConfig,
        confidence_calibrator: Arc<Mutex<ConfidenceCalibrator>>,
        pattern_memory: Arc<Mutex<PatternMemory>>,
    ) -> Self {
        Self {
            config,
            confidence_calibrator,
            pattern_memory,
        }
    }

    /// Calculate base score using aggressive approach
    fn calculate_base_score(&self, candidate: &PremintCandidate) -> (u8, HashMap<String, f64>) {
        let mut score = 0u8;
        let mut feature_scores = HashMap::new();

        // Aggressive scoring emphasizes opportunity and growth

        // Jito bundle - high bonus (aggressive on MEV advantage)
        if candidate.is_jito_bundle.unwrap_or(false) {
            score = score.saturating_add(15);
            feature_scores.insert("jito_bundle".to_string(), 15.0);
        }

        // Program bonus - aggressive, more accepting
        if candidate.program.contains("pump.fun") {
            score = score.saturating_add(20);
            feature_scores.insert("program_quality".to_string(), 20.0);
        } else if candidate.program.contains("raydium")
            || candidate.program.contains("orca")
            || candidate.program.contains("meteora")
        {
            score = score.saturating_add(15);
            feature_scores.insert("program_quality".to_string(), 15.0);
        } else {
            // Even unknown programs get some credit in aggressive mode
            score = score.saturating_add(5);
            feature_scores.insert("program_quality".to_string(), 5.0);
        }

        // Instruction quality - aggressive prefers any activity
        if let Some(ref instr) = candidate.instruction_summary {
            if instr.contains("swap") || instr.contains("buy") || instr.contains("trade") {
                score = score.saturating_add(10);
                feature_scores.insert("instruction_quality".to_string(), 10.0);
            } else {
                score = score.saturating_add(5);
                feature_scores.insert("instruction_quality".to_string(), 5.0);
            }
        }

        // Start with moderate base for aggressive (easier to pass threshold)
        score = score.saturating_add(20);
        feature_scores.insert("base_score".to_string(), 20.0);

        (score, feature_scores)
    }

    /// Extract pattern observations (same as conservative)
    fn extract_observations(
        &self,
        candidate: &PremintCandidate,
        historical_data: Option<&[PremintCandidate]>,
    ) -> Vec<Observation> {
        let mut observations = Vec::new();

        let current_obs = Observation {
            features: vec![
                Self::normalize_slot(candidate.slot),
                Self::normalize_timestamp(candidate.timestamp),
                if candidate.is_jito_bundle.unwrap_or(false) {
                    1.0
                } else {
                    0.0
                },
                Self::normalize_program(&candidate.program),
            ],
            timestamp: candidate.timestamp,
        };
        observations.push(current_obs);

        if let Some(history) = historical_data {
            for hist in history.iter().take(5) {
                let hist_obs = Observation {
                    features: vec![
                        Self::normalize_slot(hist.slot),
                        Self::normalize_timestamp(hist.timestamp),
                        if hist.is_jito_bundle.unwrap_or(false) {
                            1.0
                        } else {
                            0.0
                        },
                        Self::normalize_program(&hist.program),
                    ],
                    timestamp: hist.timestamp,
                };
                observations.push(hist_obs);
            }
        }

        observations
    }

    fn normalize_slot(slot: u64) -> f32 {
        let normalized = (slot % 1_000_000) as f64 / 1_000_000.0;
        normalized.min(1.0).max(0.0) as f32
    }

    fn normalize_timestamp(timestamp: u64) -> f32 {
        let seconds_in_day = 86400u64;
        let normalized = (timestamp % seconds_in_day) as f64 / seconds_in_day as f64;
        normalized.min(1.0).max(0.0) as f32
    }

    fn normalize_program(program: &str) -> f32 {
        let hash: u32 = program
            .bytes()
            .fold(0u32, |acc, b| acc.wrapping_add(b as u32));
        (hash % 100) as f32 / 100.0
    }
}

#[async_trait::async_trait]
impl OracleStrategyTrait for AggressiveOracle {
    async fn score_candidate(
        &self,
        candidate: &PremintCandidate,
        historical_data: Option<&[PremintCandidate]>,
    ) -> Result<(u8, f64, HashMap<String, f64>)> {
        debug!("AggressiveOracle scoring candidate: {}", candidate.mint);

        // Calculate base score
        let (mut score, mut feature_scores) = self.calculate_base_score(candidate);

        // Check pattern memory with aggressive adjustments
        let observations = self.extract_observations(candidate, historical_data);

        if observations.len() >= 2 {
            let pattern_memory = self.pattern_memory.lock().await;

            if let Some(pattern_match) = pattern_memory.find_match(&observations) {
                let confidence = pattern_match.pattern.confidence;

                // Aggressive: trust patterns with moderate confidence
                if confidence > 0.6 {
                    if pattern_match.predicted_outcome {
                        // Aggressive: larger boost for good patterns
                        let boost =
                            (confidence as f64 * 20.0 * self.config.pattern_boost_multiplier) as u8;
                        score = score.saturating_add(boost);
                        feature_scores.insert("pattern_boost".to_string(), boost as f64);
                        info!(
                            "Aggressive pattern boost: +{} pts (conf: {:.2})",
                            boost, confidence
                        );
                    } else {
                        // Aggressive: smaller penalty for bad patterns
                        let penalty =
                            (confidence as f64 * 30.0 * self.config.pattern_penalty_multiplier)
                                as u8;
                        score = score.saturating_sub(penalty);
                        feature_scores.insert("pattern_penalty".to_string(), -(penalty as f64));
                        info!(
                            "Aggressive pattern penalty: -{} pts (conf: {:.2})",
                            penalty, confidence
                        );
                    }
                    feature_scores.insert("pattern_confidence".to_string(), confidence as f64);
                }
            }
        }

        // Apply minimum threshold (more lenient)
        if score < self.config.min_score_threshold {
            // Aggressive: still give some score even if below threshold
            score = (score as f64 * 0.7) as u8;
        }

        // Calculate confidence (aggressive: higher confidence in general)
        let base_confidence = (score as f64 / 100.0).min(1.0);
        let mut calibrator = self.confidence_calibrator.lock().await;
        let calibrated_confidence = calibrator.calibrate_confidence(base_confidence);

        Ok((score, calibrated_confidence, feature_scores))
    }

    fn get_config(&self) -> &StrategyConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_candidate(
        mint: &str,
        slot: u64,
        is_jito: bool,
        program: &str,
    ) -> PremintCandidate {
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
    async fn test_conservative_oracle_creation() {
        let calibrator = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
        let pattern_memory = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));

        let oracle = ConservativeOracle::new(calibrator, pattern_memory);
        assert_eq!(oracle.get_config().min_score_threshold, 70);
    }

    #[tokio::test]
    async fn test_aggressive_oracle_creation() {
        let calibrator = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
        let pattern_memory = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));

        let oracle = AggressiveOracle::new(calibrator, pattern_memory);
        assert_eq!(oracle.get_config().min_score_threshold, 50);
    }

    #[tokio::test]
    async fn test_conservative_vs_aggressive_scoring() {
        let calibrator_c = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
        let pattern_memory_c = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));
        let conservative = ConservativeOracle::new(calibrator_c, pattern_memory_c);

        let calibrator_a = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
        let pattern_memory_a = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));
        let aggressive = AggressiveOracle::new(calibrator_a, pattern_memory_a);

        let candidate = create_test_candidate("mint1", 1000, true, "pump.fun");

        let (conservative_score, conservative_conf, _) = conservative
            .score_candidate(&candidate, None)
            .await
            .unwrap();

        let (aggressive_score, aggressive_conf, _) =
            aggressive.score_candidate(&candidate, None).await.unwrap();

        // Aggressive should generally give higher scores
        assert!(aggressive_score >= conservative_score || conservative_score == 0);

        println!(
            "Conservative: score={}, conf={:.3} | Aggressive: score={}, conf={:.3}",
            conservative_score, conservative_conf, aggressive_score, aggressive_conf
        );
    }

    #[tokio::test]
    async fn test_conservative_rejects_low_scores() {
        let calibrator = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
        let pattern_memory = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));
        let conservative = ConservativeOracle::new(calibrator, pattern_memory);

        // Create a candidate that should score low
        let candidate = create_test_candidate("mint1", 1000, false, "unknown_program");

        let (score, _, _) = conservative
            .score_candidate(&candidate, None)
            .await
            .unwrap();

        // Conservative should reject low scores
        assert_eq!(
            score, 0,
            "Conservative oracle should reject low-scoring candidates"
        );
    }

    #[tokio::test]
    async fn test_aggressive_accepts_moderate_scores() {
        let calibrator = Arc::new(Mutex::new(ConfidenceCalibrator::new()));
        let pattern_memory = Arc::new(Mutex::new(PatternMemory::with_capacity(100)));
        let aggressive = AggressiveOracle::new(calibrator, pattern_memory);

        // Create a candidate that should score moderately
        let candidate = create_test_candidate("mint1", 1000, false, "unknown_program");

        let (score, _, _) = aggressive.score_candidate(&candidate, None).await.unwrap();

        // Aggressive should still give some score
        assert!(
            score > 0,
            "Aggressive oracle should accept moderate candidates"
        );
    }
}
