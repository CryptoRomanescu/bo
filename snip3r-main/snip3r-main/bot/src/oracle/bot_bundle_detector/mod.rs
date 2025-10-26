//! Bot Activity & Bundle Detection Module
//!
//! Detects bot activity, transaction bundling, and coordinated trading in Solana memecoin launches.
//!
//! # Key Metrics
//! - Bot activity percentage: 70-85% of first transactions are typically bots
//! - Organic ratio: >30% = healthy token; <15% = avoid/penalize
//! - Bundle detection: Coordinated pumps or manipulation
//! - High-frequency patterns: Repeated addresses, temporal clustering
//!
//! # Analysis Latency
//! - Target: <10s for complete analysis
//! - Uses parallel async analysis for speed
//!
//! # Reference
//! Based on CryptoRomanescu analysis of memecoin launches

pub mod analyzer;
pub mod detector;
pub mod types;

pub use analyzer::BotActivityAnalyzer;
pub use detector::BotBundleDetector;
pub use types::*;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Configuration for bot and bundle detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotBundleConfig {
    /// Maximum analysis window from token launch (seconds)
    pub analysis_window_secs: u64,

    /// Minimum transactions to analyze before making decision
    pub min_transactions: usize,

    /// Maximum transactions to analyze
    pub max_transactions: usize,

    /// Bot detection threshold - if transaction timing < this, likely bot (milliseconds)
    pub bot_timing_threshold_ms: u64,

    /// Bundle detection - max time between transactions in a bundle (milliseconds)
    pub bundle_time_window_ms: u64,

    /// Minimum bundle size to consider as coordinated
    pub min_bundle_size: usize,

    /// High-frequency threshold - transactions per second to flag as bot
    pub high_frequency_threshold: f64,

    /// Healthy organic ratio threshold (e.g., 0.30 = 30%)
    pub healthy_organic_threshold: f64,

    /// Avoid threshold - bot percentage above which to avoid (e.g., 0.85 = 85%)
    pub avoid_bot_threshold: f64,

    /// Timeout for analysis
    pub analysis_timeout: Duration,
}

impl Default for BotBundleConfig {
    fn default() -> Self {
        Self {
            analysis_window_secs: 300, // 5 minutes from launch
            min_transactions: 20,
            max_transactions: 1000,
            bot_timing_threshold_ms: 100, // <100ms between deploy and tx = likely bot
            bundle_time_window_ms: 500,   // Txs within 500ms = potential bundle
            min_bundle_size: 3,
            high_frequency_threshold: 10.0, // >10 tx/s from same address = bot
            healthy_organic_threshold: 0.30, // 30% organic = healthy
            avoid_bot_threshold: 0.85,      // 85% bot = avoid
            analysis_timeout: Duration::from_secs(10),
        }
    }
}

/// Classification of token based on bot/organic analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenClassification {
    /// Organic trading dominates (>30% organic)
    Organic,
    /// Mixed activity (30%-70% organic)
    Mixed,
    /// Bot-dominated (<30% organic, but <85% bots)
    BotDominated,
    /// Heavily manipulated (>85% bots)
    HighlyManipulated,
    /// Bundle-coordinated pump detected
    BundleCoordinated,
}

impl TokenClassification {
    /// Get a human-readable description
    pub fn description(&self) -> &str {
        match self {
            Self::Organic => "Healthy organic trading activity",
            Self::Mixed => "Mixed organic and automated trading",
            Self::BotDominated => "Bot-dominated but not critically manipulated",
            Self::HighlyManipulated => "Highly manipulated, avoid",
            Self::BundleCoordinated => "Coordinated bundle pump detected",
        }
    }

    /// Whether this classification should trigger avoidance
    pub fn should_avoid(&self) -> bool {
        matches!(self, Self::HighlyManipulated | Self::BundleCoordinated)
    }

    /// Get risk score (0.0-1.0, higher = more risky)
    pub fn risk_score(&self) -> f64 {
        match self {
            Self::Organic => 0.1,
            Self::Mixed => 0.4,
            Self::BotDominated => 0.7,
            Self::HighlyManipulated => 0.95,
            Self::BundleCoordinated => 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = BotBundleConfig::default();
        assert_eq!(config.min_transactions, 20);
        assert_eq!(config.healthy_organic_threshold, 0.30);
        assert_eq!(config.avoid_bot_threshold, 0.85);
    }

    #[test]
    fn test_classification_should_avoid() {
        assert!(!TokenClassification::Organic.should_avoid());
        assert!(!TokenClassification::Mixed.should_avoid());
        assert!(!TokenClassification::BotDominated.should_avoid());
        assert!(TokenClassification::HighlyManipulated.should_avoid());
        assert!(TokenClassification::BundleCoordinated.should_avoid());
    }

    #[test]
    fn test_classification_risk_scores() {
        assert!(TokenClassification::Organic.risk_score() < 0.2);
        assert!(TokenClassification::Mixed.risk_score() < 0.5);
        assert!(TokenClassification::BotDominated.risk_score() > 0.6);
        assert!(TokenClassification::HighlyManipulated.risk_score() > 0.9);
        assert_eq!(TokenClassification::BundleCoordinated.risk_score(), 1.0);
    }
}
