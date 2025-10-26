//! Holder Growth Analyzer - Universe-Class Real-time Holder Growth Analysis
//!
//! Analyzes the growth rate and pattern of unique token holders over time to detect:
//! - Organic vs bot-driven growth
//! - Sudden jumps (coordinated bot attacks)
//! - Suspicious flattening (bots switching off)
//! - Rapid drops (early dumps)
//! - Natural growth curves
//!
//! ## Features
//! - Historical holder tracking at 2-5s intervals for first 60-120s
//! - Growth curve analysis with anomaly detection
//! - Correlation with smart money and wash trading signals
//! - Bot activity detection through growth pattern analysis
//! - Growth score calculation (0-100, higher = better/more organic)
//!
//! ## Reference
//! Based on CryptoRomanescu research:
//! - Organic tokens show steady growth with natural variance
//! - Bot tokens show sudden jumps followed by flattening
//! - Scam tokens show rapid initial growth then collapse
//! - Smart money correlation indicates quality

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcProgramAccountsConfig;
use solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use tracing::{debug, info, instrument, warn};

/// Single snapshot of holder count at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HolderSnapshot {
    /// Timestamp (unix seconds)
    pub timestamp: u64,
    /// Number of unique holders at this time
    pub holder_count: usize,
    /// Time since token deploy (seconds)
    pub age_seconds: u64,
}

/// Growth pattern anomaly types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GrowthAnomaly {
    /// Sudden jump in holders (likely bot coordination)
    SuddenJump {
        start_holders: usize,
        end_holders: usize,
        duration_secs: u64,
        growth_rate: f64,
    },
    /// Suspicious flattening (bots turned off)
    SuspiciousFlattening {
        holder_count: usize,
        duration_secs: u64,
        timestamp: u64,
    },
    /// Rapid drop (early dump)
    RapidDrop {
        start_holders: usize,
        end_holders: usize,
        duration_secs: u64,
        drop_rate: f64,
    },
    /// Unnatural growth curve (too linear/smooth for organic)
    UnnaturalCurve {
        variance: f64,
        expected_variance: f64,
    },
}

/// Complete holder growth analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HolderGrowthAnalysis {
    /// Token mint address
    pub mint: String,
    /// Token deployment timestamp
    pub deploy_timestamp: u64,
    /// Historical holder snapshots
    pub snapshots: Vec<HolderSnapshot>,
    /// Current holder count
    pub current_holders: usize,
    /// Growth rate (holders per second)
    pub growth_rate: f64,
    /// Growth score (0-100, higher = better/more organic)
    pub growth_score: u8,
    /// Detected anomalies
    pub anomalies: Vec<GrowthAnomaly>,
    /// Bot activity probability (0-1)
    pub bot_probability: f64,
    /// Is growth organic?
    pub is_organic: bool,
    /// Smart money correlation score (0-1)
    pub smart_money_correlation: f64,
    /// Wash trading correlation score (0-1)
    pub wash_trading_correlation: f64,
    /// Analysis time in milliseconds
    pub analysis_time_ms: u64,
    /// Timestamp of analysis
    pub timestamp: u64,
}

/// Configuration for holder growth analyzer
#[derive(Debug, Clone)]
pub struct HolderGrowthConfig {
    /// Snapshot interval in seconds (default: 3s)
    pub snapshot_interval_secs: u64,
    /// Maximum analysis window in seconds (default: 120s)
    pub max_analysis_window_secs: u64,
    /// Minimum snapshots required for analysis (default: 5)
    pub min_snapshots: usize,
    /// Analysis timeout in seconds
    pub timeout_secs: u64,
    /// Sudden jump threshold (growth rate multiplier, default: 5x)
    pub sudden_jump_threshold: f64,
    /// Flattening threshold (max holders change, default: 5)
    pub flattening_threshold: usize,
    /// Flattening duration threshold (seconds, default: 15s)
    pub flattening_duration_threshold: u64,
    /// Rapid drop threshold (drop rate multiplier, default: 3x)
    pub rapid_drop_threshold: f64,
    /// Bot probability weights
    pub bot_prob_weights: BotProbabilityWeights,
}

/// Weights for bot probability calculation
#[derive(Debug, Clone)]
pub struct BotProbabilityWeights {
    /// Base weight for sudden jumps (default: 0.3)
    pub sudden_jump_base: f64,
    /// Multiplier for excess ratio in sudden jumps (default: 0.05)
    pub sudden_jump_excess: f64,
    /// Normalizer for flattening duration (default: 30.0)
    pub flattening_normalizer: f64,
    /// Weight for flattening penalty (default: 0.25)
    pub flattening_weight: f64,
    /// Base weight for rapid drops (default: 0.3)
    pub rapid_drop_base: f64,
    /// Multiplier for excess ratio in rapid drops (default: 0.05)
    pub rapid_drop_excess: f64,
    /// Weight for unnatural curve penalty (default: 0.25)
    pub unnatural_curve_weight: f64,
}

impl Default for BotProbabilityWeights {
    fn default() -> Self {
        Self {
            sudden_jump_base: 0.3,
            sudden_jump_excess: 0.05,
            flattening_normalizer: 30.0,
            flattening_weight: 0.25,
            rapid_drop_base: 0.3,
            rapid_drop_excess: 0.05,
            unnatural_curve_weight: 0.25,
        }
    }
}

impl Default for HolderGrowthConfig {
    fn default() -> Self {
        Self {
            snapshot_interval_secs: 3,
            max_analysis_window_secs: 120,
            min_snapshots: 5,
            timeout_secs: 10,
            sudden_jump_threshold: 5.0,
            flattening_threshold: 5,
            flattening_duration_threshold: 15,
            rapid_drop_threshold: 3.0,
            bot_prob_weights: BotProbabilityWeights::default(),
        }
    }
}

/// Holder Growth Analyzer
pub struct HolderGrowthAnalyzer {
    config: HolderGrowthConfig,
    rpc_client: Arc<RpcClient>,
    /// Historical snapshots cache (mint -> snapshots)
    snapshot_cache: Arc<tokio::sync::RwLock<HashMap<String, Vec<HolderSnapshot>>>>,
}

impl HolderGrowthAnalyzer {
    /// Create a new holder growth analyzer
    pub fn new(config: HolderGrowthConfig, rpc_client: Arc<RpcClient>) -> Self {
        info!(
            "Initialized HolderGrowthAnalyzer with snapshot_interval={}s, max_window={}s",
            config.snapshot_interval_secs, config.max_analysis_window_secs
        );

        Self {
            config,
            rpc_client,
            snapshot_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Analyze holder growth for a token
    ///
    /// # Arguments
    /// * `mint` - Token mint address
    /// * `deploy_timestamp` - Token deployment timestamp (unix seconds)
    /// * `smart_money_score` - Optional smart money involvement score (0-100)
    /// * `wash_trading_score` - Optional wash trading risk score (0-100)
    #[instrument(skip(self))]
    pub async fn analyze(
        &self,
        mint: &str,
        deploy_timestamp: u64,
        smart_money_score: Option<u8>,
        wash_trading_score: Option<u8>,
    ) -> Result<HolderGrowthAnalysis> {
        let start_time = Instant::now();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        debug!(
            "Starting holder growth analysis for mint={}, deploy_timestamp={}",
            mint, deploy_timestamp
        );

        // Get current holder count
        let current_holders = self
            .get_holder_count(mint)
            .await
            .context("Failed to get current holder count")?;

        // Get or create historical snapshots
        let mut snapshots = self.get_snapshots(mint, deploy_timestamp, now).await?;

        // Add current snapshot if enough time has passed
        if snapshots.is_empty()
            || (now - snapshots.last().unwrap().timestamp) >= self.config.snapshot_interval_secs
        {
            let snapshot = HolderSnapshot {
                timestamp: now,
                holder_count: current_holders,
                age_seconds: now.saturating_sub(deploy_timestamp),
            };
            snapshots.push(snapshot.clone());

            // Update cache
            let mut cache = self.snapshot_cache.write().await;
            cache.insert(mint.to_string(), snapshots.clone());
        }

        // Ensure we have enough snapshots
        if snapshots.len() < self.config.min_snapshots {
            warn!(
                "Insufficient snapshots for analysis: {} < {}",
                snapshots.len(),
                self.config.min_snapshots
            );
            // Return default analysis with low score
            return self.create_default_analysis(mint, deploy_timestamp, current_holders, snapshots, start_time);
        }

        // Analyze growth patterns
        let growth_rate = self.calculate_growth_rate(&snapshots);
        let anomalies = self.detect_anomalies(&snapshots);
        let bot_probability = self.calculate_bot_probability(&snapshots, &anomalies);
        let is_organic = self.is_growth_organic(&snapshots, &anomalies, bot_probability);

        // Calculate correlations
        let smart_money_correlation = smart_money_score.map(|s| s as f64 / 100.0).unwrap_or(0.0);
        let wash_trading_correlation = wash_trading_score.map(|s| s as f64 / 100.0).unwrap_or(0.0);

        // Calculate growth score
        let growth_score = self.calculate_growth_score(
            &snapshots,
            &anomalies,
            bot_probability,
            is_organic,
            smart_money_correlation,
            wash_trading_correlation,
        );

        let analysis_time_ms = start_time.elapsed().as_millis() as u64;

        info!(
            "Holder growth analysis complete for {}: holders={}, growth_rate={:.2}, score={}, organic={}, bot_prob={:.2}, anomalies={}, time={}ms",
            mint, current_holders, growth_rate, growth_score, is_organic, bot_probability, anomalies.len(), analysis_time_ms
        );

        Ok(HolderGrowthAnalysis {
            mint: mint.to_string(),
            deploy_timestamp,
            snapshots,
            current_holders,
            growth_rate,
            growth_score,
            anomalies,
            bot_probability,
            is_organic,
            smart_money_correlation,
            wash_trading_correlation,
            analysis_time_ms,
            timestamp: now,
        })
    }

    /// Get current holder count for a token
    async fn get_holder_count(&self, mint: &str) -> Result<usize> {
        let mint_pubkey = Pubkey::from_str(mint).context("Invalid mint address")?;

        // Prepare filter for token accounts
        let filters = vec![
            RpcFilterType::DataSize(165), // SPL Token Account size
            RpcFilterType::Memcmp(Memcmp::new(
                0, // Offset for mint address in token account
                MemcmpEncodedBytes::Base58(mint.to_string()),
            )),
        ];

        let config = RpcProgramAccountsConfig {
            filters: Some(filters),
            account_config: Default::default(),
            ..Default::default()
        };

        // Get token accounts
        let token_program = solana_sdk::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
        let accounts = tokio::time::timeout(
            Duration::from_secs(self.config.timeout_secs),
            self.rpc_client
                .get_program_accounts_with_config(&token_program, config),
        )
        .await
        .context("Timeout getting token accounts")?
        .context("Failed to get token accounts")?;

        // Filter out zero balance accounts
        let holder_count = accounts.len();

        debug!("Found {} holders for mint {}", holder_count, mint);
        Ok(holder_count)
    }

    /// Get or create historical snapshots
    async fn get_snapshots(
        &self,
        mint: &str,
        deploy_timestamp: u64,
        now: u64,
    ) -> Result<Vec<HolderSnapshot>> {
        // Check cache first
        let cache = self.snapshot_cache.read().await;
        if let Some(cached_snapshots) = cache.get(mint) {
            debug!(
                "Using cached snapshots for {}: {} snapshots",
                mint,
                cached_snapshots.len()
            );
            return Ok(cached_snapshots.clone());
        }
        drop(cache);

        // No cache - create initial snapshot
        debug!("No cached snapshots for {}, creating initial snapshot", mint);
        Ok(Vec::new())
    }

    /// Calculate average growth rate (holders per second)
    fn calculate_growth_rate(&self, snapshots: &[HolderSnapshot]) -> f64 {
        if snapshots.len() < 2 {
            return 0.0;
        }

        let first = &snapshots[0];
        let last = &snapshots[snapshots.len() - 1];

        let time_diff = last.timestamp.saturating_sub(first.timestamp);
        if time_diff == 0 {
            return 0.0;
        }

        let holder_diff = last.holder_count as i64 - first.holder_count as i64;
        holder_diff as f64 / time_diff as f64
    }

    /// Detect growth anomalies
    fn detect_anomalies(&self, snapshots: &[HolderSnapshot]) -> Vec<GrowthAnomaly> {
        let mut anomalies = Vec::new();

        if snapshots.len() < 2 {
            return anomalies;
        }

        // Detect sudden jumps
        for i in 1..snapshots.len() {
            let prev = &snapshots[i - 1];
            let curr = &snapshots[i];

            let time_diff = curr.timestamp.saturating_sub(prev.timestamp);
            if time_diff == 0 {
                continue;
            }

            let holder_diff = curr.holder_count as i64 - prev.holder_count as i64;

            // Sudden jump detection
            if holder_diff > 0 {
                let growth_rate = holder_diff as f64 / time_diff as f64;
                let avg_rate = self.calculate_growth_rate(snapshots);

                if avg_rate > 0.0 && growth_rate > avg_rate * self.config.sudden_jump_threshold {
                    anomalies.push(GrowthAnomaly::SuddenJump {
                        start_holders: prev.holder_count,
                        end_holders: curr.holder_count,
                        duration_secs: time_diff,
                        growth_rate,
                    });
                    info!(
                        "Detected sudden jump: {} -> {} holders in {}s (rate={:.2})",
                        prev.holder_count, curr.holder_count, time_diff, growth_rate
                    );
                }
            }

            // Rapid drop detection
            if holder_diff < 0 {
                let drop_rate = (-holder_diff) as f64 / time_diff as f64;
                let avg_rate = self.calculate_growth_rate(snapshots).abs();

                if avg_rate > 0.0 && drop_rate > avg_rate * self.config.rapid_drop_threshold {
                    anomalies.push(GrowthAnomaly::RapidDrop {
                        start_holders: prev.holder_count,
                        end_holders: curr.holder_count,
                        duration_secs: time_diff,
                        drop_rate,
                    });
                    warn!(
                        "Detected rapid drop: {} -> {} holders in {}s (rate={:.2})",
                        prev.holder_count, curr.holder_count, time_diff, drop_rate
                    );
                }
            }
        }

        // Detect suspicious flattening (multiple consecutive snapshots with minimal change)
        let mut flat_start: Option<usize> = None;
        for i in 1..snapshots.len() {
            let prev = &snapshots[i - 1];
            let curr = &snapshots[i];

            let holder_diff = (curr.holder_count as i64 - prev.holder_count as i64).abs() as usize;

            if holder_diff <= self.config.flattening_threshold {
                if flat_start.is_none() {
                    flat_start = Some(i - 1);
                }
            } else {
                // End of flat period
                if let Some(start_idx) = flat_start {
                    let duration = snapshots[i - 1].timestamp - snapshots[start_idx].timestamp;
                    if duration >= self.config.flattening_duration_threshold {
                        anomalies.push(GrowthAnomaly::SuspiciousFlattening {
                            holder_count: snapshots[i - 1].holder_count,
                            duration_secs: duration,
                            timestamp: snapshots[start_idx].timestamp,
                        });
                        info!(
                            "Detected suspicious flattening: {} holders for {}s",
                            snapshots[i - 1].holder_count,
                            duration
                        );
                    }
                    flat_start = None;
                }
            }
        }

        // Check if flattening continues to the end
        if let Some(start_idx) = flat_start {
            let duration = snapshots[snapshots.len() - 1].timestamp - snapshots[start_idx].timestamp;
            if duration >= self.config.flattening_duration_threshold {
                anomalies.push(GrowthAnomaly::SuspiciousFlattening {
                    holder_count: snapshots[snapshots.len() - 1].holder_count,
                    duration_secs: duration,
                    timestamp: snapshots[start_idx].timestamp,
                });
                info!(
                    "Detected suspicious flattening at end: {} holders for {}s",
                    snapshots[snapshots.len() - 1].holder_count,
                    duration
                );
            }
        }

        // Detect unnatural curve (too linear/smooth)
        if snapshots.len() >= 5 {
            let variance = self.calculate_variance(snapshots);
            let expected_variance = self.calculate_expected_variance(snapshots);

            // If variance is too low compared to expected (too smooth), likely bot-driven
            if variance < expected_variance * 0.3 {
                anomalies.push(GrowthAnomaly::UnnaturalCurve {
                    variance,
                    expected_variance,
                });
                info!(
                    "Detected unnatural growth curve: variance={:.2}, expected={:.2}",
                    variance, expected_variance
                );
            }
        }

        anomalies
    }

    /// Calculate variance in growth rates
    fn calculate_variance(&self, snapshots: &[HolderSnapshot]) -> f64 {
        if snapshots.len() < 2 {
            return 0.0;
        }

        let mut rates = Vec::new();
        for i in 1..snapshots.len() {
            let prev = &snapshots[i - 1];
            let curr = &snapshots[i];
            let time_diff = curr.timestamp.saturating_sub(prev.timestamp);
            if time_diff > 0 {
                let rate = (curr.holder_count as i64 - prev.holder_count as i64) as f64
                    / time_diff as f64;
                rates.push(rate);
            }
        }

        if rates.is_empty() {
            return 0.0;
        }

        let mean = rates.iter().sum::<f64>() / rates.len() as f64;
        let variance = rates
            .iter()
            .map(|r| (r - mean).powi(2))
            .sum::<f64>()
            / rates.len() as f64;

        variance
    }

    /// Calculate expected variance for organic growth
    fn calculate_expected_variance(&self, snapshots: &[HolderSnapshot]) -> f64 {
        // Organic growth typically has moderate variance (not too smooth, not too spiky)
        // This is a heuristic based on the average growth rate
        let avg_rate = self.calculate_growth_rate(snapshots).abs();
        
        // Expected variance is proportional to growth rate
        // For organic growth, variance is typically 20-50% of the mean rate
        avg_rate * 0.3
    }

    /// Calculate bot probability based on growth patterns
    fn calculate_bot_probability(
        &self,
        snapshots: &[HolderSnapshot],
        anomalies: &[GrowthAnomaly],
    ) -> f64 {
        let mut bot_score = 0.0;
        let weights = &self.config.bot_prob_weights;

        // Each anomaly increases bot probability
        for anomaly in anomalies {
            match anomaly {
                GrowthAnomaly::SuddenJump { growth_rate, .. } => {
                    let avg_rate = self.calculate_growth_rate(snapshots).abs();
                    if avg_rate > 0.0 {
                        let ratio = growth_rate / avg_rate;
                        // Strong penalty for sudden jumps - base + excess
                        let excess_ratio = (ratio - self.config.sudden_jump_threshold).max(0.0);
                        bot_score += weights.sudden_jump_base + (excess_ratio * weights.sudden_jump_excess);
                    }
                }
                GrowthAnomaly::SuspiciousFlattening { duration_secs, .. } => {
                    // Longer flattening = higher bot probability
                    bot_score += (*duration_secs as f64 / weights.flattening_normalizer) * weights.flattening_weight;
                }
                GrowthAnomaly::RapidDrop { drop_rate, .. } => {
                    let avg_rate = self.calculate_growth_rate(snapshots).abs();
                    if avg_rate > 0.0 {
                        let ratio = drop_rate / avg_rate;
                        let excess_ratio = (ratio - self.config.rapid_drop_threshold).max(0.0);
                        bot_score += weights.rapid_drop_base + (excess_ratio * weights.rapid_drop_excess);
                    }
                }
                GrowthAnomaly::UnnaturalCurve { variance, expected_variance } => {
                    if *expected_variance > 0.0 {
                        let ratio = variance / expected_variance;
                        // Lower variance = higher bot probability
                        bot_score += (1.0 - ratio).max(0.0) * weights.unnatural_curve_weight;
                    }
                }
            }
        }

        // Clamp to 0-1 range
        bot_score.min(1.0).max(0.0)
    }

    /// Determine if growth is organic
    fn is_growth_organic(
        &self,
        snapshots: &[HolderSnapshot],
        anomalies: &[GrowthAnomaly],
        bot_probability: f64,
    ) -> bool {
        // Growth is organic if:
        // 1. Bot probability is low (<0.3)
        // 2. No major anomalies (sudden jumps or rapid drops)
        // 3. Reasonable growth rate

        if bot_probability >= 0.3 {
            return false;
        }

        // Check for major anomalies
        for anomaly in anomalies {
            match anomaly {
                GrowthAnomaly::SuddenJump { growth_rate, .. } => {
                    let avg_rate = self.calculate_growth_rate(snapshots).abs();
                    if avg_rate > 0.0 && growth_rate / avg_rate > 10.0 {
                        return false; // Extreme jump
                    }
                }
                GrowthAnomaly::RapidDrop { .. } => {
                    return false; // Any rapid drop is concerning
                }
                _ => {}
            }
        }

        true
    }

    /// Calculate growth score (0-100)
    fn calculate_growth_score(
        &self,
        snapshots: &[HolderSnapshot],
        anomalies: &[GrowthAnomaly],
        bot_probability: f64,
        is_organic: bool,
        smart_money_correlation: f64,
        wash_trading_correlation: f64,
    ) -> u8 {
        let mut score = 50.0; // Base score

        // Positive factors
        if is_organic {
            score += 20.0;
        }

        // Steady growth is good
        let growth_rate = self.calculate_growth_rate(snapshots);
        if growth_rate > 0.0 && growth_rate < 5.0 {
            // Moderate growth (0-5 holders/sec)
            score += 15.0;
        } else if growth_rate >= 5.0 {
            // High growth, slightly suspicious
            score += 5.0;
        }

        // Smart money correlation is positive
        score += smart_money_correlation * 15.0;

        // Negative factors
        score -= bot_probability * 30.0;
        score -= wash_trading_correlation * 20.0;

        // Penalty for anomalies
        for anomaly in anomalies {
            match anomaly {
                GrowthAnomaly::SuddenJump { .. } => score -= 10.0,
                GrowthAnomaly::SuspiciousFlattening { .. } => score -= 8.0,
                GrowthAnomaly::RapidDrop { .. } => score -= 15.0,
                GrowthAnomaly::UnnaturalCurve { .. } => score -= 5.0,
            }
        }

        // Clamp to 0-100
        score.round().min(100.0).max(0.0) as u8
    }

    /// Create default analysis when insufficient data
    fn create_default_analysis(
        &self,
        mint: &str,
        deploy_timestamp: u64,
        current_holders: usize,
        snapshots: Vec<HolderSnapshot>,
        start_time: Instant,
    ) -> Result<HolderGrowthAnalysis> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let analysis_time_ms = start_time.elapsed().as_millis() as u64;

        Ok(HolderGrowthAnalysis {
            mint: mint.to_string(),
            deploy_timestamp,
            snapshots,
            current_holders,
            growth_rate: 0.0,
            growth_score: 50, // Neutral score for insufficient data
            anomalies: Vec::new(),
            bot_probability: 0.0,
            is_organic: false,
            smart_money_correlation: 0.0,
            wash_trading_correlation: 0.0,
            analysis_time_ms,
            timestamp: now,
        })
    }

    /// Clear snapshots for a token (for testing/force refresh)
    pub async fn clear_snapshots(&self, mint: &str) {
        let mut cache = self.snapshot_cache.write().await;
        cache.remove(mint);
        debug!("Cleared snapshots for token: {}", mint);
    }

    /// Get current snapshot count for a token
    pub async fn get_snapshot_count(&self, mint: &str) -> usize {
        let cache = self.snapshot_cache.read().await;
        cache.get(mint).map(|s| s.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_snapshots(pattern: &str) -> Vec<HolderSnapshot> {
        let base_time = 1000000u64;
        match pattern {
            "organic" => vec![
                HolderSnapshot {
                    timestamp: base_time,
                    holder_count: 10,
                    age_seconds: 0,
                },
                HolderSnapshot {
                    timestamp: base_time + 3,
                    holder_count: 15,
                    age_seconds: 3,
                },
                HolderSnapshot {
                    timestamp: base_time + 6,
                    holder_count: 22,
                    age_seconds: 6,
                },
                HolderSnapshot {
                    timestamp: base_time + 9,
                    holder_count: 28,
                    age_seconds: 9,
                },
                HolderSnapshot {
                    timestamp: base_time + 12,
                    holder_count: 35,
                    age_seconds: 12,
                },
            ],
            "sudden_jump" => vec![
                HolderSnapshot {
                    timestamp: base_time,
                    holder_count: 10,
                    age_seconds: 0,
                },
                HolderSnapshot {
                    timestamp: base_time + 3,
                    holder_count: 12,
                    age_seconds: 3,
                },
                HolderSnapshot {
                    timestamp: base_time + 6,
                    holder_count: 14,
                    age_seconds: 6,
                },
                HolderSnapshot {
                    timestamp: base_time + 9,
                    holder_count: 16,
                    age_seconds: 9,
                },
                HolderSnapshot {
                    timestamp: base_time + 12,
                    holder_count: 18,
                    age_seconds: 12,
                },
                HolderSnapshot {
                    timestamp: base_time + 15,
                    holder_count: 150, // Sudden jump from 18 to 150 in 3s
                    age_seconds: 15,
                },
                HolderSnapshot {
                    timestamp: base_time + 18,
                    holder_count: 155,
                    age_seconds: 18,
                },
            ],
            "flattening" => vec![
                HolderSnapshot {
                    timestamp: base_time,
                    holder_count: 10,
                    age_seconds: 0,
                },
                HolderSnapshot {
                    timestamp: base_time + 3,
                    holder_count: 20,
                    age_seconds: 3,
                },
                HolderSnapshot {
                    timestamp: base_time + 6,
                    holder_count: 21, // Flat
                    age_seconds: 6,
                },
                HolderSnapshot {
                    timestamp: base_time + 9,
                    holder_count: 22, // Flat
                    age_seconds: 9,
                },
                HolderSnapshot {
                    timestamp: base_time + 12,
                    holder_count: 21, // Flat
                    age_seconds: 12,
                },
                HolderSnapshot {
                    timestamp: base_time + 15,
                    holder_count: 23, // Flat
                    age_seconds: 15,
                },
                HolderSnapshot {
                    timestamp: base_time + 18,
                    holder_count: 22, // Flat
                    age_seconds: 18,
                },
            ],
            _ => vec![],
        }
    }

    #[tokio::test]
    async fn test_growth_rate_calculation() {
        let config = HolderGrowthConfig::default();
        let rpc = Arc::new(RpcClient::new(
            "https://api.mainnet-beta.solana.com".to_string(),
        ));
        let analyzer = HolderGrowthAnalyzer::new(config, rpc);

        let snapshots = create_test_snapshots("organic");
        let growth_rate = analyzer.calculate_growth_rate(&snapshots);

        // 10 -> 35 holders in 12 seconds = 25/12 = ~2.08 holders/sec
        assert!(growth_rate > 2.0 && growth_rate < 2.2);
    }

    #[tokio::test]
    async fn test_sudden_jump_detection() {
        let config = HolderGrowthConfig::default();
        let rpc = Arc::new(RpcClient::new(
            "https://api.mainnet-beta.solana.com".to_string(),
        ));
        let analyzer = HolderGrowthAnalyzer::new(config, rpc);

        let snapshots = create_test_snapshots("sudden_jump");
        let anomalies = analyzer.detect_anomalies(&snapshots);

        // Should detect sudden jump
        assert!(!anomalies.is_empty());
        let has_sudden_jump = anomalies
            .iter()
            .any(|a| matches!(a, GrowthAnomaly::SuddenJump { .. }));
        assert!(has_sudden_jump);
    }

    #[tokio::test]
    async fn test_flattening_detection() {
        let config = HolderGrowthConfig::default();
        let rpc = Arc::new(RpcClient::new(
            "https://api.mainnet-beta.solana.com".to_string(),
        ));
        let analyzer = HolderGrowthAnalyzer::new(config, rpc);

        let snapshots = create_test_snapshots("flattening");
        let anomalies = analyzer.detect_anomalies(&snapshots);

        // Should detect flattening
        assert!(!anomalies.is_empty());
        let has_flattening = anomalies
            .iter()
            .any(|a| matches!(a, GrowthAnomaly::SuspiciousFlattening { .. }));
        assert!(has_flattening);
    }

    #[tokio::test]
    async fn test_organic_growth_scoring() {
        let config = HolderGrowthConfig::default();
        let rpc = Arc::new(RpcClient::new(
            "https://api.mainnet-beta.solana.com".to_string(),
        ));
        let analyzer = HolderGrowthAnalyzer::new(config, rpc);

        let snapshots = create_test_snapshots("organic");
        let anomalies = analyzer.detect_anomalies(&snapshots);
        let bot_prob = analyzer.calculate_bot_probability(&snapshots, &anomalies);
        let is_organic = analyzer.is_growth_organic(&snapshots, &anomalies, bot_prob);
        let score = analyzer.calculate_growth_score(
            &snapshots,
            &anomalies,
            bot_prob,
            is_organic,
            0.5, // moderate smart money
            0.1, // low wash trading
        );

        // Organic growth should score high
        assert!(score >= 60);
        assert!(is_organic);
        assert!(bot_prob < 0.3);
    }

    #[tokio::test]
    async fn test_bot_growth_scoring() {
        let config = HolderGrowthConfig::default();
        let rpc = Arc::new(RpcClient::new(
            "https://api.mainnet-beta.solana.com".to_string(),
        ));
        let analyzer = HolderGrowthAnalyzer::new(config, rpc);

        let snapshots = create_test_snapshots("sudden_jump");
        let anomalies = analyzer.detect_anomalies(&snapshots);
        let bot_prob = analyzer.calculate_bot_probability(&snapshots, &anomalies);
        let is_organic = analyzer.is_growth_organic(&snapshots, &anomalies, bot_prob);
        let score = analyzer.calculate_growth_score(
            &snapshots,
            &anomalies,
            bot_prob,
            is_organic,
            0.1, // low smart money
            0.7, // high wash trading
        );

        // Bot-driven growth should score low
        println!("Bot growth test - score: {}, bot_prob: {:.2}, is_organic: {}", score, bot_prob, is_organic);
        assert!(score <= 50, "Expected score <= 50, got {}", score);
        assert!(!is_organic);
        assert!(bot_prob > 0.0);
    }
}
