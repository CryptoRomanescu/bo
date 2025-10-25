//! MetaArbiter - Dynamic strategy selection based on market regime and performance

use anyhow::Result;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::oracle::ensemble::types::{
    ArbiterDecision, EnsembleConfig, OracleStrategy, StrategyPerformance,
};
use crate::oracle::types::MarketRegime;

/// Performance tracking for a single trade
#[derive(Debug, Clone)]
pub struct TradeOutcome {
    pub strategy: OracleStrategy,
    pub regime: MarketRegime,
    pub was_successful: bool,
    pub pnl: f64,
    pub confidence: f64,
    pub timestamp: u64,
}

/// MetaArbiter dynamically selects optimal oracle strategy
pub struct MetaArbiter {
    /// Current market regime (shared with MarketRegimeDetector)
    current_regime: Arc<RwLock<MarketRegime>>,

    /// Ensemble configuration
    config: EnsembleConfig,

    /// Performance history per strategy per regime
    performance_history: HashMap<(OracleStrategy, MarketRegime), VecDeque<TradeOutcome>>,

    /// Currently selected strategy
    current_strategy: OracleStrategy,

    /// Timestamp of last strategy switch
    last_switch_timestamp: u64,

    /// Performance metrics per strategy per regime
    performance_metrics: HashMap<(OracleStrategy, MarketRegime), StrategyPerformance>,
}

impl MetaArbiter {
    /// Create a new MetaArbiter
    pub fn new(current_regime: Arc<RwLock<MarketRegime>>, config: EnsembleConfig) -> Self {
        Self {
            current_regime,
            config,
            performance_history: HashMap::new(),
            current_strategy: OracleStrategy::Conservative, // Start conservative
            last_switch_timestamp: 0,
            performance_metrics: HashMap::new(),
        }
    }

    /// Get the current recommended strategy
    pub async fn get_strategy(&self) -> ArbiterDecision {
        let regime = *self.current_regime.read().await;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Check if we should use ensemble voting
        let use_ensemble = self.should_use_ensemble(&regime).await;

        if use_ensemble {
            return ArbiterDecision {
                selected_strategy: OracleStrategy::Ensemble,
                reason: format!(
                    "Using ensemble voting for {} regime",
                    format_regime(&regime)
                ),
                confidence: 0.8,
                market_regime: regime,
                timestamp,
            };
        }

        // Check cooldown period
        if timestamp - self.last_switch_timestamp < self.config.strategy_switch_cooldown_secs {
            return ArbiterDecision {
                selected_strategy: self.current_strategy,
                reason: format!("Cooldown period active, keeping {}", self.current_strategy),
                confidence: 0.7,
                market_regime: regime,
                timestamp,
            };
        }

        // Select strategy based on regime and performance
        let selected = self.select_strategy_for_regime(&regime);

        let (confidence, reason) = if selected == self.current_strategy {
            (
                0.9,
                format!(
                    "Continuing with {} for {} regime",
                    selected,
                    format_regime(&regime)
                ),
            )
        } else {
            (
                0.75,
                format!(
                    "Switching to {} for {} regime",
                    selected,
                    format_regime(&regime)
                ),
            )
        };

        ArbiterDecision {
            selected_strategy: selected,
            reason,
            confidence,
            market_regime: regime,
            timestamp,
        }
    }

    /// Update strategy selection
    pub async fn update_strategy(&mut self, decision: &ArbiterDecision) {
        if decision.selected_strategy != self.current_strategy
            && decision.selected_strategy != OracleStrategy::Ensemble
        {
            info!(
                "MetaArbiter strategy switch: {} -> {} (reason: {})",
                self.current_strategy, decision.selected_strategy, decision.reason
            );
            self.current_strategy = decision.selected_strategy;
            self.last_switch_timestamp = decision.timestamp;
        }
    }

    /// Record a trade outcome for performance tracking
    pub async fn record_trade_outcome(&mut self, outcome: TradeOutcome) {
        let key = (outcome.strategy, outcome.regime);

        // Add to history
        let history = self
            .performance_history
            .entry(key)
            .or_insert_with(VecDeque::new);
        history.push_back(outcome.clone());

        // Maintain window size
        while history.len() > self.config.performance_window_trades {
            history.pop_front();
        }

        // Update metrics
        self.update_metrics(outcome.strategy, outcome.regime);

        debug!(
            "Recorded trade outcome: strategy={}, regime={:?}, success={}, pnl={:.3}",
            outcome.strategy, outcome.regime, outcome.was_successful, outcome.pnl
        );
    }

    /// Update performance metrics for a strategy-regime combination
    fn update_metrics(&mut self, strategy: OracleStrategy, regime: MarketRegime) {
        let key = (strategy, regime);

        let history = match self.performance_history.get(&key) {
            Some(h) if !h.is_empty() => h,
            _ => return,
        };

        let trade_count = history.len();
        let wins = history.iter().filter(|t| t.was_successful).count();
        let total_profit: f64 = history
            .iter()
            .filter(|t| t.was_successful)
            .map(|t| t.pnl)
            .sum();
        let total_loss: f64 = history
            .iter()
            .filter(|t| !t.was_successful)
            .map(|t| t.pnl.abs())
            .sum();
        let avg_confidence: f64 =
            history.iter().map(|t| t.confidence).sum::<f64>() / trade_count as f64;

        let win_rate = if trade_count > 0 {
            wins as f64 / trade_count as f64
        } else {
            0.0
        };

        let profit_factor = if total_loss > 0.0 {
            total_profit / total_loss
        } else if total_profit > 0.0 {
            total_profit
        } else {
            1.0
        };

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let performance = StrategyPerformance {
            strategy,
            win_rate,
            profit_factor,
            avg_confidence,
            trade_count,
            regime,
            last_updated: timestamp,
        };

        self.performance_metrics.insert(key, performance);
    }

    /// Select best strategy for current market regime
    fn select_strategy_for_regime(&self, regime: &MarketRegime) -> OracleStrategy {
        // Default regime-based selection
        let default_strategy = match regime {
            MarketRegime::Bullish => OracleStrategy::Aggressive,
            MarketRegime::Bearish => OracleStrategy::Conservative,
            MarketRegime::Choppy => OracleStrategy::Conservative, // Risk-off in choppy
            MarketRegime::HighCongestion => OracleStrategy::Conservative, // Risk-off with high fees
            MarketRegime::LowActivity => OracleStrategy::Conservative, // Wait for better conditions
        };

        // Check if we have performance data to override default
        let conservative_perf = self
            .performance_metrics
            .get(&(OracleStrategy::Conservative, *regime));
        let aggressive_perf = self
            .performance_metrics
            .get(&(OracleStrategy::Aggressive, *regime));

        match (conservative_perf, aggressive_perf) {
            (Some(c), Some(a)) if c.trade_count >= 10 && a.trade_count >= 10 => {
                // Both have sufficient data, compare performance
                let c_score = c.profit_factor * c.win_rate;
                let a_score = a.profit_factor * a.win_rate;

                if a_score > c_score * 1.2 {
                    // Aggressive significantly better
                    OracleStrategy::Aggressive
                } else if c_score > a_score * 1.2 {
                    // Conservative significantly better
                    OracleStrategy::Conservative
                } else {
                    // Similar performance, use default
                    default_strategy
                }
            }
            _ => {
                // Insufficient data, use regime-based default
                default_strategy
            }
        }
    }

    /// Determine if ensemble voting should be used
    async fn should_use_ensemble(&self, regime: &MarketRegime) -> bool {
        // Use ensemble in choppy markets where uncertainty is high
        if *regime == MarketRegime::Choppy {
            return true;
        }

        // Check performance uncertainty
        let conservative_perf = self
            .performance_metrics
            .get(&(OracleStrategy::Conservative, *regime));
        let aggressive_perf = self
            .performance_metrics
            .get(&(OracleStrategy::Aggressive, *regime));

        match (conservative_perf, aggressive_perf) {
            (Some(c), Some(a)) if c.trade_count >= 5 && a.trade_count >= 5 => {
                // If both strategies have similar performance, use ensemble
                let c_score = c.profit_factor * c.win_rate;
                let a_score = a.profit_factor * a.win_rate;

                let ratio = c_score.max(a_score) / c_score.min(a_score).max(0.01);

                // Use ensemble if strategies are within 20% of each other
                ratio < 1.2
            }
            _ => {
                // Not enough data, don't use ensemble yet
                false
            }
        }
    }

    /// Get performance metrics for a specific strategy and regime
    pub fn get_performance(
        &self,
        strategy: OracleStrategy,
        regime: MarketRegime,
    ) -> Option<&StrategyPerformance> {
        self.performance_metrics.get(&(strategy, regime))
    }

    /// Get all performance metrics
    pub fn get_all_performance(
        &self,
    ) -> &HashMap<(OracleStrategy, MarketRegime), StrategyPerformance> {
        &self.performance_metrics
    }

    /// Get trade count for a strategy-regime combination
    pub fn get_trade_count(&self, strategy: OracleStrategy, regime: MarketRegime) -> usize {
        self.performance_history
            .get(&(strategy, regime))
            .map(|h| h.len())
            .unwrap_or(0)
    }
}

fn format_regime(regime: &MarketRegime) -> &str {
    match regime {
        MarketRegime::Bullish => "Bullish",
        MarketRegime::Bearish => "Bearish",
        MarketRegime::Choppy => "Choppy",
        MarketRegime::HighCongestion => "HighCongestion",
        MarketRegime::LowActivity => "LowActivity",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_arbiter() -> MetaArbiter {
        let regime = Arc::new(RwLock::new(MarketRegime::LowActivity));
        let config = EnsembleConfig::default();
        MetaArbiter::new(regime, config)
    }

    #[tokio::test]
    async fn test_arbiter_creation() {
        let arbiter = create_test_arbiter();
        let decision = arbiter.get_strategy().await;
        assert_eq!(decision.selected_strategy, OracleStrategy::Conservative);
    }

    #[tokio::test]
    async fn test_strategy_selection_bullish() {
        let regime = Arc::new(RwLock::new(MarketRegime::Bullish));
        let config = EnsembleConfig::default();
        let arbiter = MetaArbiter::new(regime.clone(), config);

        let decision = arbiter.get_strategy().await;
        // Should recommend aggressive for bullish market
        assert!(
            decision.selected_strategy == OracleStrategy::Aggressive
                || decision.selected_strategy == OracleStrategy::Ensemble
        );
    }

    #[tokio::test]
    async fn test_strategy_selection_bearish() {
        let regime = Arc::new(RwLock::new(MarketRegime::Bearish));
        let config = EnsembleConfig::default();
        let arbiter = MetaArbiter::new(regime.clone(), config);

        let decision = arbiter.get_strategy().await;
        // Should recommend conservative for bearish market
        assert_eq!(decision.selected_strategy, OracleStrategy::Conservative);
    }

    #[tokio::test]
    async fn test_record_trade_outcome() {
        let mut arbiter = create_test_arbiter();

        let outcome = TradeOutcome {
            strategy: OracleStrategy::Conservative,
            regime: MarketRegime::LowActivity,
            was_successful: true,
            pnl: 0.5,
            confidence: 0.8,
            timestamp: 1000,
        };

        arbiter.record_trade_outcome(outcome).await;

        let count =
            arbiter.get_trade_count(OracleStrategy::Conservative, MarketRegime::LowActivity);
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_performance_metrics_update() {
        let mut arbiter = create_test_arbiter();

        // Record multiple trades
        for i in 0..10 {
            let outcome = TradeOutcome {
                strategy: OracleStrategy::Conservative,
                regime: MarketRegime::LowActivity,
                was_successful: i % 2 == 0, // 50% win rate
                pnl: if i % 2 == 0 { 0.3 } else { -0.2 },
                confidence: 0.7,
                timestamp: 1000 + i,
            };
            arbiter.record_trade_outcome(outcome).await;
        }

        let perf = arbiter.get_performance(OracleStrategy::Conservative, MarketRegime::LowActivity);

        assert!(perf.is_some());
        let perf = perf.unwrap();
        assert_eq!(perf.trade_count, 10);
        assert!((perf.win_rate - 0.5).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_cooldown_period() {
        let mut arbiter = create_test_arbiter();

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let decision = ArbiterDecision {
            selected_strategy: OracleStrategy::Aggressive,
            reason: "Test switch".to_string(),
            confidence: 0.8,
            market_regime: MarketRegime::Bullish,
            timestamp,
        };

        arbiter.update_strategy(&decision).await;

        // Immediately check strategy - should respect cooldown
        let new_decision = arbiter.get_strategy().await;
        assert_eq!(new_decision.selected_strategy, OracleStrategy::Aggressive);
        assert!(new_decision.reason.contains("Cooldown"));
    }

    #[tokio::test]
    async fn test_ensemble_selection_choppy_market() {
        let regime = Arc::new(RwLock::new(MarketRegime::Choppy));
        let config = EnsembleConfig::default();
        let arbiter = MetaArbiter::new(regime.clone(), config);

        let decision = arbiter.get_strategy().await;
        // Choppy markets should use ensemble
        assert_eq!(decision.selected_strategy, OracleStrategy::Ensemble);
    }
}
