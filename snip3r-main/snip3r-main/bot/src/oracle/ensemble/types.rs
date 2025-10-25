//! Types for the Quantum Ensemble Oracle System

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::oracle::types::{FeatureWeights, MarketRegime, ScoreThresholds};

/// Configuration for the ensemble system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnsembleConfig {
    /// Whether ensemble is enabled
    pub enabled: bool,
    /// Minimum confidence threshold to use voting (below this, use MetaArbiter's single strategy)
    pub voting_threshold: f64,
    /// Cooldown period between strategy switches (in seconds)
    pub strategy_switch_cooldown_secs: u64,
    /// Number of recent trades to track for performance evaluation
    pub performance_window_trades: usize,
    /// Strength of Bayesian prior (higher = more weight on historical performance)
    pub bayesian_prior_strength: f64,
}

impl Default for EnsembleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            voting_threshold: 0.6,
            strategy_switch_cooldown_secs: 300, // 5 minutes
            performance_window_trades: 50,
            bayesian_prior_strength: 0.3,
        }
    }
}

/// Oracle strategy types
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum OracleStrategy {
    /// Conservative strategy - risk-averse with strict thresholds
    Conservative,
    /// Aggressive strategy - opportunity-seeking with looser thresholds
    Aggressive,
    /// Ensemble voting - aggregate multiple strategies
    Ensemble,
}

impl std::fmt::Display for OracleStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OracleStrategy::Conservative => write!(f, "Conservative"),
            OracleStrategy::Aggressive => write!(f, "Aggressive"),
            OracleStrategy::Ensemble => write!(f, "Ensemble"),
        }
    }
}

/// Performance metrics for a specific strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyPerformance {
    /// The strategy being tracked
    pub strategy: OracleStrategy,
    /// Win rate (percentage)
    pub win_rate: f64,
    /// Profit factor (total profits / total losses)
    pub profit_factor: f64,
    /// Average confidence of predictions
    pub avg_confidence: f64,
    /// Number of trades executed
    pub trade_count: usize,
    /// Market regime during these trades
    pub regime: MarketRegime,
    /// Timestamp of last update
    pub last_updated: u64,
}

impl Default for StrategyPerformance {
    fn default() -> Self {
        Self {
            strategy: OracleStrategy::Conservative,
            win_rate: 0.0,
            profit_factor: 1.0,
            avg_confidence: 0.5,
            trade_count: 0,
            regime: MarketRegime::LowActivity,
            last_updated: 0,
        }
    }
}

/// Result of Bayesian voting from multiple oracles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VotingResult {
    /// Final aggregated score
    pub final_score: u8,
    /// Final aggregated confidence
    pub final_confidence: f64,
    /// Individual votes from each strategy (strategy -> (score, confidence))
    pub strategy_votes: HashMap<OracleStrategy, (u8, f64)>,
    /// Weights applied to each strategy in voting
    pub strategy_weights: HashMap<OracleStrategy, f64>,
    /// Uncertainty measure (variance in predictions)
    pub uncertainty: f64,
    /// Reason for the final decision
    pub reason: String,
}

/// Configuration for a specific oracle strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// Feature weights for this strategy
    pub weights: FeatureWeights,
    /// Score thresholds for this strategy
    pub thresholds: ScoreThresholds,
    /// Minimum score to consider a candidate
    pub min_score_threshold: u8,
    /// Pattern confidence boost multiplier
    pub pattern_boost_multiplier: f64,
    /// Pattern confidence penalty multiplier
    pub pattern_penalty_multiplier: f64,
}

impl StrategyConfig {
    /// Create a conservative strategy configuration
    pub fn conservative() -> Self {
        Self {
            weights: FeatureWeights {
                liquidity: 0.28,           // High weight on liquidity
                holder_distribution: 0.23, // High weight on distribution
                volume_growth: 0.09,       // Lower weight on growth
                holder_growth: 0.05,       // Lower weight on growth
                price_change: 0.05,        // Lower weight on volatility
                jito_bundle_presence: 0.09,
                creator_sell_speed: 0.09,
                metadata_quality: 0.05,
                social_activity: 0.00, // Minimal social weight
                dex_trending: 0.04,    // Conservative weight on trending
                dex_momentum: 0.03,    // Conservative weight on momentum
            },
            thresholds: ScoreThresholds {
                min_liquidity_sol: 20.0,             // Stricter liquidity requirement
                whale_threshold: 0.10,               // Stricter whale concentration
                volume_growth_threshold: 3.0,        // Higher growth needed
                holder_growth_threshold: 2.0,        // Higher growth needed
                min_metadata_quality: 0.8,           // High quality required
                creator_sell_penalty_threshold: 180, // Faster penalty trigger
                social_activity_threshold: 150.0,    // Higher social requirement
            },
            min_score_threshold: 70,         // High minimum score
            pattern_boost_multiplier: 0.7,   // Conservative pattern boost
            pattern_penalty_multiplier: 1.5, // Aggressive pattern penalty
        }
    }

    /// Create an aggressive strategy configuration
    pub fn aggressive() -> Self {
        Self {
            weights: FeatureWeights {
                liquidity: 0.13,           // Lower weight on liquidity
                holder_distribution: 0.09, // Lower weight on distribution
                volume_growth: 0.23,       // High weight on growth
                holder_growth: 0.18,       // High weight on growth
                price_change: 0.13,        // Higher weight on volatility
                jito_bundle_presence: 0.05,
                creator_sell_speed: 0.05,
                metadata_quality: 0.00,
                social_activity: 0.05, // Some social weight
                dex_trending: 0.06,    // Aggressive weight on trending
                dex_momentum: 0.03,    // Momentum weight
            },
            thresholds: ScoreThresholds {
                min_liquidity_sol: 5.0,              // Looser liquidity requirement
                whale_threshold: 0.25,               // Looser whale concentration
                volume_growth_threshold: 1.5,        // Lower growth needed
                holder_growth_threshold: 1.2,        // Lower growth needed
                min_metadata_quality: 0.5,           // Lower quality accepted
                creator_sell_penalty_threshold: 450, // Slower penalty trigger
                social_activity_threshold: 50.0,     // Lower social requirement
            },
            min_score_threshold: 50,         // Lower minimum score
            pattern_boost_multiplier: 1.3,   // Aggressive pattern boost
            pattern_penalty_multiplier: 0.7, // Conservative pattern penalty
        }
    }
}

/// MetaArbiter decision on which strategy to use
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbiterDecision {
    /// Selected strategy
    pub selected_strategy: OracleStrategy,
    /// Reason for the selection
    pub reason: String,
    /// Confidence in this decision (0.0-1.0)
    pub confidence: f64,
    /// Current market regime
    pub market_regime: MarketRegime,
    /// Timestamp of decision
    pub timestamp: u64,
}
