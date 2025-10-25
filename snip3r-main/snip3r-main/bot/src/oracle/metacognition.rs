//! Lightweight Metacognitive Awareness System
//!
//! This module implements a memory-efficient confidence calibration system
//! with zero external dependencies, designed for real-time operation (<1ms).
//!
//! Key Features:
//! - Zero External Dependencies: Only standard Rust libraries
//! - Memory Efficient: Sliding window of last 100 decisions (~800 bytes)
//! - CPU Friendly: Simple statistics, no heavy ML
//! - Real-time: Calculations in <1ms
//! - Small Footprint: ~5-10KB RAM per instance

use std::collections::VecDeque;
use std::time::Instant;

/// Maximum number of decisions to keep in the sliding window
const WINDOW_SIZE: usize = 100;

/// Represents a single decision outcome for confidence calibration
#[derive(Debug, Clone, Copy)]
pub struct DecisionOutcome {
    /// Predicted confidence level (0.0 to 1.0)
    pub predicted_confidence: f64,
    /// Predicted score (0-100)
    pub predicted_score: u8,
    /// Whether the decision was successful (true) or failed (false)
    pub was_successful: bool,
    /// Actual profit/loss (normalized to -1.0 to 1.0 range)
    pub actual_outcome: f64,
    /// Timestamp of the decision
    pub timestamp: u64,
}

impl DecisionOutcome {
    /// Create a new decision outcome record
    pub fn new(
        predicted_confidence: f64,
        predicted_score: u8,
        was_successful: bool,
        actual_outcome: f64,
        timestamp: u64,
    ) -> Self {
        Self {
            predicted_confidence: predicted_confidence.clamp(0.0, 1.0),
            predicted_score: predicted_score.min(100),
            was_successful,
            actual_outcome: actual_outcome.clamp(-1.0, 1.0),
            timestamp,
        }
    }
}

/// Statistics computed from the decision history
#[derive(Debug, Clone, Default)]
pub struct CalibrationStats {
    /// Number of decisions in the current window
    pub decision_count: usize,
    /// Overall accuracy rate (0.0 to 1.0)
    pub accuracy: f64,
    /// Mean absolute calibration error
    pub calibration_error: f64,
    /// Average predicted confidence
    pub avg_predicted_confidence: f64,
    /// Average actual success rate
    pub avg_success_rate: f64,
    /// Confidence bias (positive = overconfident, negative = underconfident)
    pub confidence_bias: f64,
    /// Time taken for last calculation (nanoseconds)
    pub calculation_time_ns: u128,
}

/// Lightweight confidence calibration system using sliding window
pub struct ConfidenceCalibrator {
    /// Sliding window of last 100 decisions (~800 bytes)
    history: VecDeque<DecisionOutcome>,
    /// Cached statistics (updated on demand)
    cached_stats: Option<CalibrationStats>,
    /// Flag indicating if cache needs refresh
    cache_dirty: bool,
}

impl ConfidenceCalibrator {
    /// Create a new confidence calibrator with empty history
    pub fn new() -> Self {
        Self {
            history: VecDeque::with_capacity(WINDOW_SIZE),
            cached_stats: None,
            cache_dirty: true,
        }
    }

    /// Add a new decision outcome to the sliding window
    ///
    /// If the window is full, the oldest decision is automatically removed.
    #[tracing::instrument(skip(self, outcome), fields(predicted_confidence = %outcome.predicted_confidence, was_successful = %outcome.was_successful))]
    pub fn add_decision(&mut self, outcome: DecisionOutcome) {
        // Remove oldest if at capacity
        if self.history.len() >= WINDOW_SIZE {
            self.history.pop_front();
        }

        self.history.push_back(outcome);
        self.cache_dirty = true;
    }

    /// Get the current number of decisions in the window
    pub fn decision_count(&self) -> usize {
        self.history.len()
    }

    /// Calculate and cache statistics from the current window
    ///
    /// This operation is designed to complete in <1ms even with a full window.
    fn calculate_stats(&mut self) -> CalibrationStats {
        let start = Instant::now();

        if self.history.is_empty() {
            return CalibrationStats {
                decision_count: 0,
                accuracy: 0.0,
                calibration_error: 0.0,
                avg_predicted_confidence: 0.0,
                avg_success_rate: 0.0,
                confidence_bias: 0.0,
                calculation_time_ns: start.elapsed().as_nanos(),
            };
        }

        let count = self.history.len() as f64;
        let mut sum_confidence = 0.0;
        let mut sum_success = 0.0;
        let mut sum_calibration_error = 0.0;

        for outcome in &self.history {
            sum_confidence += outcome.predicted_confidence;
            sum_success += if outcome.was_successful { 1.0 } else { 0.0 };

            // Calculate calibration error: |predicted - actual|
            let actual_binary = if outcome.was_successful { 1.0 } else { 0.0 };
            sum_calibration_error += (outcome.predicted_confidence - actual_binary).abs();
        }

        let avg_confidence = sum_confidence / count;
        let success_rate = sum_success / count;
        let calibration_error = sum_calibration_error / count;
        let confidence_bias = avg_confidence - success_rate;

        CalibrationStats {
            decision_count: self.history.len(),
            accuracy: success_rate,
            calibration_error,
            avg_predicted_confidence: avg_confidence,
            avg_success_rate: success_rate,
            confidence_bias,
            calculation_time_ns: start.elapsed().as_nanos(),
        }
    }

    /// Get current calibration statistics
    ///
    /// Returns cached statistics if available, otherwise calculates fresh stats.
    /// Calculation time is guaranteed to be <1ms.
    pub fn get_stats(&mut self) -> CalibrationStats {
        if self.cache_dirty || self.cached_stats.is_none() {
            let stats = self.calculate_stats();
            self.cached_stats = Some(stats.clone());
            self.cache_dirty = false;
            stats
        } else {
            self.cached_stats.as_ref().unwrap().clone()
        }
    }

    /// Calibrate a new confidence prediction based on historical performance
    ///
    /// This adjusts the raw confidence score based on observed calibration bias.
    /// Returns the calibrated confidence value (0.0 to 1.0).
    pub fn calibrate_confidence(&mut self, raw_confidence: f64) -> f64 {
        let raw_confidence = raw_confidence.clamp(0.0, 1.0);

        // If we don't have enough data, return raw confidence
        if self.history.len() < 10 {
            return raw_confidence;
        }

        let stats = self.get_stats();

        // Apply simple bias correction
        // If we're overconfident (positive bias), reduce confidence
        // If we're underconfident (negative bias), increase confidence
        let calibrated = raw_confidence - (stats.confidence_bias * 0.5);

        calibrated.clamp(0.0, 1.0)
    }

    /// Get recent decision outcomes (up to last N decisions)
    pub fn get_recent_decisions(&self, count: usize) -> Vec<DecisionOutcome> {
        self.history.iter().rev().take(count).copied().collect()
    }

    /// Clear all history and reset the calibrator
    pub fn clear(&mut self) {
        self.history.clear();
        self.cached_stats = None;
        self.cache_dirty = true;
    }

    /// Get the estimated memory footprint in bytes
    pub fn memory_footprint(&self) -> usize {
        // VecDeque overhead + (DecisionOutcome size * capacity)
        // DecisionOutcome is 8 bytes (f64) + 1 byte (u8) + 1 byte (bool) + 8 bytes (f64) + 8 bytes (u64) = ~32 bytes with padding
        // Plus VecDeque overhead
        std::mem::size_of::<VecDeque<DecisionOutcome>>()
            + (std::mem::size_of::<DecisionOutcome>() * WINDOW_SIZE)
            + std::mem::size_of::<Option<CalibrationStats>>()
    }
}

impl Default for ConfidenceCalibrator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_calibrator() {
        let calibrator = ConfidenceCalibrator::new();
        assert_eq!(calibrator.decision_count(), 0);
    }

    #[test]
    fn test_add_decision() {
        let mut calibrator = ConfidenceCalibrator::new();

        let outcome = DecisionOutcome::new(0.8, 85, true, 0.5, 1000);
        calibrator.add_decision(outcome);

        assert_eq!(calibrator.decision_count(), 1);
    }

    #[test]
    fn test_sliding_window() {
        let mut calibrator = ConfidenceCalibrator::new();

        // Add more than WINDOW_SIZE decisions
        for i in 0..150 {
            let outcome = DecisionOutcome::new(
                0.7,
                70,
                i % 2 == 0, // Alternating success/failure
                0.0,
                i as u64,
            );
            calibrator.add_decision(outcome);
        }

        // Should only keep last 100
        assert_eq!(calibrator.decision_count(), WINDOW_SIZE);
    }

    #[test]
    fn test_calculate_stats() {
        let mut calibrator = ConfidenceCalibrator::new();

        // Add 10 successful decisions with 0.8 confidence
        for i in 0..10 {
            let outcome = DecisionOutcome::new(0.8, 80, true, 0.5, i as u64);
            calibrator.add_decision(outcome);
        }

        let stats = calibrator.get_stats();

        assert_eq!(stats.decision_count, 10);
        assert!((stats.avg_predicted_confidence - 0.8).abs() < 0.01);
        assert!((stats.avg_success_rate - 1.0).abs() < 0.01);
        assert!(stats.calculation_time_ns < 1_000_000); // Less than 1ms
    }

    #[test]
    fn test_calibration_bias_overconfident() {
        let mut calibrator = ConfidenceCalibrator::new();

        // Predict high confidence but low success rate (overconfident)
        for i in 0..20 {
            let outcome = DecisionOutcome::new(
                0.9, // High predicted confidence
                90,
                i < 10, // Only 50% success rate
                if i < 10 { 0.5 } else { -0.5 },
                i as u64,
            );
            calibrator.add_decision(outcome);
        }

        let stats = calibrator.get_stats();

        // Should detect overconfidence (positive bias)
        assert!(stats.confidence_bias > 0.0);
        assert!((stats.avg_predicted_confidence - 0.9).abs() < 0.01);
        assert!((stats.avg_success_rate - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_calibration_bias_underconfident() {
        let mut calibrator = ConfidenceCalibrator::new();

        // Predict low confidence but high success rate (underconfident)
        for i in 0..20 {
            let outcome = DecisionOutcome::new(
                0.5, // Low predicted confidence
                50,
                i < 18, // 90% success rate
                if i < 18 { 0.5 } else { -0.5 },
                i as u64,
            );
            calibrator.add_decision(outcome);
        }

        let stats = calibrator.get_stats();

        // Should detect underconfidence (negative bias)
        assert!(stats.confidence_bias < 0.0);
    }

    #[test]
    fn test_calibrate_confidence() {
        let mut calibrator = ConfidenceCalibrator::new();

        // Add overconfident history
        for i in 0..20 {
            let outcome = DecisionOutcome::new(
                0.8,
                80,
                i < 10, // 50% success
                0.0,
                i as u64,
            );
            calibrator.add_decision(outcome);
        }

        // Calibrated confidence should be lower than raw
        let raw_confidence = 0.8;
        let calibrated = calibrator.calibrate_confidence(raw_confidence);

        assert!(calibrated < raw_confidence);
    }

    #[test]
    fn test_calibrate_confidence_insufficient_data() {
        let mut calibrator = ConfidenceCalibrator::new();

        // Add only a few decisions
        for i in 0..5 {
            let outcome = DecisionOutcome::new(0.8, 80, true, 0.5, i as u64);
            calibrator.add_decision(outcome);
        }

        // Should return raw confidence when insufficient data
        let raw_confidence = 0.75;
        let calibrated = calibrator.calibrate_confidence(raw_confidence);

        assert_eq!(calibrated, raw_confidence);
    }

    #[test]
    fn test_get_recent_decisions() {
        let mut calibrator = ConfidenceCalibrator::new();

        // Add 10 decisions
        for i in 0..10 {
            let outcome = DecisionOutcome::new(0.7, 70, true, 0.5, i as u64);
            calibrator.add_decision(outcome);
        }

        let recent = calibrator.get_recent_decisions(5);

        assert_eq!(recent.len(), 5);
        // Should be in reverse order (most recent first)
        assert_eq!(recent[0].timestamp, 9);
        assert_eq!(recent[4].timestamp, 5);
    }

    #[test]
    fn test_clear() {
        let mut calibrator = ConfidenceCalibrator::new();

        // Add some decisions
        for i in 0..10 {
            let outcome = DecisionOutcome::new(0.7, 70, true, 0.5, i as u64);
            calibrator.add_decision(outcome);
        }

        calibrator.clear();

        assert_eq!(calibrator.decision_count(), 0);
    }

    #[test]
    fn test_memory_footprint() {
        let calibrator = ConfidenceCalibrator::new();
        let footprint = calibrator.memory_footprint();

        // Should be reasonable (around 5-10KB as specified)
        assert!(footprint > 1000); // At least 1KB
        assert!(footprint < 15000); // Less than 15KB (some margin)

        println!(
            "Memory footprint: {} bytes (~{}KB)",
            footprint,
            footprint / 1024
        );
    }

    #[test]
    fn test_performance_under_1ms() {
        let mut calibrator = ConfidenceCalibrator::new();

        // Fill with maximum capacity
        for i in 0..WINDOW_SIZE {
            let outcome = DecisionOutcome::new(0.7, 70, i % 2 == 0, 0.0, i as u64);
            calibrator.add_decision(outcome);
        }

        // Measure calculation time
        let start = Instant::now();
        let stats = calibrator.get_stats();
        let elapsed = start.elapsed();

        // Should complete in less than 1ms
        assert!(
            elapsed.as_micros() < 1000,
            "Calculation took {}μs",
            elapsed.as_micros()
        );
        assert!(
            stats.calculation_time_ns < 1_000_000,
            "Stats calculation_time_ns: {}",
            stats.calculation_time_ns
        );
    }

    #[test]
    fn test_confidence_clamping() {
        let outcome = DecisionOutcome::new(
            1.5, // Above 1.0
            120, // Above 100
            true, 2.0, // Above 1.0
            1000,
        );

        assert_eq!(outcome.predicted_confidence, 1.0);
        assert_eq!(outcome.predicted_score, 100);
        assert_eq!(outcome.actual_outcome, 1.0);
    }

    #[test]
    fn test_negative_confidence_clamping() {
        let outcome = DecisionOutcome::new(
            -0.5, // Below 0.0
            0, false, -2.0, // Below -1.0
            1000,
        );

        assert_eq!(outcome.predicted_confidence, 0.0);
        assert_eq!(outcome.actual_outcome, -1.0);
    }
}
