//! Bot Activity Analyzer
//!
//! Analyzes transaction patterns to detect bot activity and high-frequency trading

use super::types::*;
use super::BotBundleConfig;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use tracing::{debug, instrument};

/// Analyzer for bot activity detection
pub struct BotActivityAnalyzer {
    config: BotBundleConfig,
}

impl BotActivityAnalyzer {
    /// Create a new bot activity analyzer
    pub fn new(config: BotBundleConfig) -> Self {
        Self { config }
    }

    /// Analyze transactions to detect bot activity
    #[instrument(skip(self, transactions))]
    pub fn analyze_bot_activity(
        &self,
        transactions: &[TransactionTiming],
        deploy_timestamp: u64,
    ) -> Result<BotMetrics> {
        if transactions.is_empty() {
            return Ok(BotMetrics {
                bot_transaction_count: 0,
                unique_bot_addresses: 0,
                avg_bot_response_time_ms: 0.0,
                instant_transaction_percentage: 0.0,
                high_frequency_bots: 0,
            });
        }

        let mut bot_count = 0;
        let mut bot_addresses = std::collections::HashSet::new();
        let mut instant_tx_count = 0;
        let mut total_bot_response_time = 0u64;

        // Analyze each transaction for bot characteristics
        for tx in transactions {
            // Check if transaction was suspiciously fast after deploy
            if tx.time_since_deploy_ms < self.config.bot_timing_threshold_ms {
                bot_count += 1;
                bot_addresses.insert(tx.address.clone());
                total_bot_response_time += tx.time_since_deploy_ms;
                instant_tx_count += 1;
            }
        }

        // Calculate metrics
        let avg_bot_response_time_ms = if bot_count > 0 {
            total_bot_response_time as f64 / bot_count as f64
        } else {
            0.0
        };

        let instant_transaction_percentage =
            instant_tx_count as f64 / transactions.len() as f64;

        // Detect high-frequency bots
        let repeated_addresses = self.detect_repeated_addresses(transactions)?;
        let high_frequency_bots = repeated_addresses
            .iter()
            .filter(|addr| addr.transactions_per_second > self.config.high_frequency_threshold)
            .count();

        debug!(
            "Bot activity analysis: {} bots, {:.1}% instant txs, {} high-freq",
            bot_count,
            instant_transaction_percentage * 100.0,
            high_frequency_bots
        );

        Ok(BotMetrics {
            bot_transaction_count: bot_count,
            unique_bot_addresses: bot_addresses.len(),
            avg_bot_response_time_ms,
            instant_transaction_percentage,
            high_frequency_bots,
        })
    }

    /// Detect repeated addresses (potential bots)
    #[instrument(skip(self, transactions))]
    pub fn detect_repeated_addresses(
        &self,
        transactions: &[TransactionTiming],
    ) -> Result<Vec<RepeatedAddress>> {
        if transactions.is_empty() {
            return Ok(vec![]);
        }

        // Group transactions by address
        let mut address_txs: HashMap<String, Vec<&TransactionTiming>> = HashMap::new();
        for tx in transactions {
            address_txs
                .entry(tx.address.clone())
                .or_insert_with(Vec::new)
                .push(tx);
        }

        let mut repeated = Vec::new();

        for (address, txs) in address_txs {
            if txs.len() < 2 {
                continue; // Not repeated
            }

            // Sort by timestamp
            let mut sorted_txs = txs.clone();
            sorted_txs.sort_by_key(|tx| tx.timestamp);

            let first = sorted_txs.first().unwrap();
            let last = sorted_txs.last().unwrap();

            // Calculate time window and transactions per second
            let duration_secs = (last.timestamp - first.timestamp).num_seconds() as f64;
            let duration_secs = duration_secs.max(1.0); // Avoid division by zero
            let transactions_per_second = sorted_txs.len() as f64 / duration_secs;

            // Calculate average time between transactions
            let mut time_diffs = Vec::new();
            for i in 1..sorted_txs.len() {
                let diff = (sorted_txs[i].timestamp - sorted_txs[i - 1].timestamp)
                    .num_milliseconds() as f64;
                time_diffs.push(diff);
            }
            let avg_time_between_txs_ms = if !time_diffs.is_empty() {
                time_diffs.iter().sum::<f64>() / time_diffs.len() as f64
            } else {
                0.0
            };

            // Calculate bot probability
            let bot_probability = self.calculate_bot_probability(
                sorted_txs.len(),
                transactions_per_second,
                avg_time_between_txs_ms,
            );

            repeated.push(RepeatedAddress {
                address,
                transaction_count: sorted_txs.len(),
                transactions_per_second,
                avg_time_between_txs_ms,
                first_transaction: first.timestamp,
                last_transaction: last.timestamp,
                bot_probability,
            });
        }

        // Sort by transaction count (descending)
        repeated.sort_by(|a, b| b.transaction_count.cmp(&a.transaction_count));

        Ok(repeated)
    }

    /// Calculate bot probability based on transaction patterns
    fn calculate_bot_probability(
        &self,
        tx_count: usize,
        txs_per_second: f64,
        avg_time_between_txs_ms: f64,
    ) -> f64 {
        let mut probability: f64 = 0.0;

        // High transaction count
        if tx_count > 10 {
            probability += 0.3;
        }
        if tx_count > 20 {
            probability += 0.2;
        }

        // High frequency
        if txs_per_second > 5.0 {
            probability += 0.3;
        }
        if txs_per_second > self.config.high_frequency_threshold {
            probability += 0.2;
        }

        // Very consistent timing (bot-like)
        if avg_time_between_txs_ms < 200.0 && tx_count > 5 {
            probability += 0.3;
        }

        probability.min(1.0)
    }

    /// Detect suspicious clusters of coordinated activity
    #[instrument(skip(self, transactions))]
    pub fn detect_suspicious_clusters(
        &self,
        transactions: &[TransactionTiming],
    ) -> Result<Vec<SuspiciousCluster>> {
        if transactions.len() < self.config.min_bundle_size {
            return Ok(vec![]);
        }

        let mut clusters = Vec::new();
        let mut cluster_id = 0;

        // Sort transactions by timestamp
        let mut sorted_txs = transactions.to_vec();
        sorted_txs.sort_by_key(|tx| tx.timestamp);

        // Sliding window to find clusters
        let mut i = 0;
        while i < sorted_txs.len() {
            let start = &sorted_txs[i];
            let mut cluster_txs = vec![&sorted_txs[i]];
            let mut addresses = std::collections::HashSet::new();
            addresses.insert(start.address.clone());

            // Find all transactions within time window
            let mut j = i + 1;
            while j < sorted_txs.len() {
                let current = &sorted_txs[j];
                let time_diff = (current.timestamp - start.timestamp).num_milliseconds() as u64;

                if time_diff <= self.config.bundle_time_window_ms {
                    cluster_txs.push(current);
                    addresses.insert(current.address.clone());
                    j += 1;
                } else {
                    break;
                }
            }

            // If cluster is large enough, mark as suspicious
            if cluster_txs.len() >= self.config.min_bundle_size {
                let time_window_ms = (cluster_txs.last().unwrap().timestamp
                    - cluster_txs.first().unwrap().timestamp)
                    .num_milliseconds() as u64;

                // Calculate suspicion score
                let suspicion_score = self.calculate_cluster_suspicion(
                    cluster_txs.len(),
                    addresses.len(),
                    time_window_ms,
                );

                if suspicion_score > 0.5 {
                    let reason = if addresses.len() > 1 {
                        format!(
                            "Coordinated: {} txs from {} addresses in {}ms",
                            cluster_txs.len(),
                            addresses.len(),
                            time_window_ms
                        )
                    } else {
                        format!(
                            "High-frequency: {} txs from 1 address in {}ms",
                            cluster_txs.len(),
                            time_window_ms
                        )
                    };

                    clusters.push(SuspiciousCluster {
                        id: cluster_id,
                        addresses: addresses.into_iter().collect(),
                        transaction_count: cluster_txs.len(),
                        time_window_ms,
                        suspicion_score,
                        reason,
                    });

                    cluster_id += 1;
                }

                // Move to next potential cluster (skip overlapping)
                i = j;
            } else {
                i += 1;
            }
        }

        debug!("Detected {} suspicious clusters", clusters.len());
        Ok(clusters)
    }

    /// Calculate cluster suspicion score
    fn calculate_cluster_suspicion(
        &self,
        tx_count: usize,
        unique_addresses: usize,
        time_window_ms: u64,
    ) -> f64 {
        let mut suspicion: f64 = 0.0;

        // Large cluster
        if tx_count >= 10 {
            suspicion += 0.4;
        } else if tx_count >= self.config.min_bundle_size {
            suspicion += 0.2;
        }

        // Coordinated (multiple addresses)
        if unique_addresses > 1 {
            suspicion += 0.3;
        }

        // Very tight timing
        if time_window_ms < 100 {
            suspicion += 0.5;
        } else if time_window_ms < self.config.bundle_time_window_ms {
            suspicion += 0.3;
        }

        suspicion.min(1.0)
    }

    /// Calculate organic ratio (1.0 - bot_percentage)
    pub fn calculate_organic_ratio(&self, bot_metrics: &BotMetrics, total_txs: usize) -> f64 {
        if total_txs == 0 {
            return 0.0;
        }

        let bot_percentage = bot_metrics.bot_transaction_count as f64 / total_txs as f64;
        1.0 - bot_percentage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_transaction(
        address: &str,
        timestamp: DateTime<Utc>,
        time_since_deploy_ms: u64,
    ) -> TransactionTiming {
        TransactionTiming {
            signature: format!("sig_{}", address),
            address: address.to_string(),
            timestamp,
            time_since_deploy_ms,
            slot: 1000,
            in_bundle: false,
            bundle_id: None,
        }
    }

    #[test]
    fn test_bot_activity_detection() {
        let config = BotBundleConfig::default();
        let analyzer = BotActivityAnalyzer::new(config);

        let base_time = Utc::now();
        let transactions = vec![
            create_test_transaction("bot1", base_time, 50),  // Bot (fast)
            create_test_transaction("bot2", base_time, 80),  // Bot (fast)
            create_test_transaction("human1", base_time, 5000), // Human (slow)
            create_test_transaction("bot3", base_time, 90),  // Bot (fast)
        ];

        let metrics = analyzer.analyze_bot_activity(&transactions, 0).unwrap();
        
        assert_eq!(metrics.bot_transaction_count, 3);
        assert_eq!(metrics.unique_bot_addresses, 3);
        assert!(metrics.avg_bot_response_time_ms < 100.0);
        assert!(metrics.instant_transaction_percentage > 0.5);
    }

    #[test]
    fn test_repeated_address_detection() {
        let config = BotBundleConfig::default();
        let analyzer = BotActivityAnalyzer::new(config);

        let base_time = Utc::now();
        let transactions = vec![
            create_test_transaction("bot1", base_time, 50),
            create_test_transaction("bot1", base_time + chrono::Duration::milliseconds(100), 150),
            create_test_transaction("bot1", base_time + chrono::Duration::milliseconds(200), 250),
            create_test_transaction("bot1", base_time + chrono::Duration::milliseconds(300), 350),
            create_test_transaction("bot1", base_time + chrono::Duration::milliseconds(400), 450),
            create_test_transaction("bot1", base_time + chrono::Duration::milliseconds(500), 550),
            create_test_transaction("human1", base_time, 5000),
        ];

        let repeated = analyzer.detect_repeated_addresses(&transactions).unwrap();
        
        assert_eq!(repeated.len(), 1);
        assert_eq!(repeated[0].address, "bot1");
        assert_eq!(repeated[0].transaction_count, 6);
        assert!(repeated[0].bot_probability > 0.5, "Expected bot probability > 0.5, got {}", repeated[0].bot_probability);
    }

    #[test]
    fn test_organic_ratio_calculation() {
        let config = BotBundleConfig::default();
        let analyzer = BotActivityAnalyzer::new(config);

        let metrics = BotMetrics {
            bot_transaction_count: 85,
            unique_bot_addresses: 20,
            avg_bot_response_time_ms: 50.0,
            instant_transaction_percentage: 0.85,
            high_frequency_bots: 5,
        };

        let organic_ratio = analyzer.calculate_organic_ratio(&metrics, 100);
        assert!((organic_ratio - 0.15).abs() < 0.01); // Allow small floating point difference
    }

    #[test]
    fn test_cluster_detection() {
        let config = BotBundleConfig::default();
        let min_bundle_size = config.min_bundle_size; // Store before move
        let analyzer = BotActivityAnalyzer::new(config);

        let base_time = Utc::now();
        let transactions = vec![
            create_test_transaction("addr1", base_time, 50),
            create_test_transaction("addr2", base_time + chrono::Duration::milliseconds(50), 100),
            create_test_transaction("addr3", base_time + chrono::Duration::milliseconds(100), 150),
            create_test_transaction("addr4", base_time + chrono::Duration::milliseconds(150), 200),
        ];

        let clusters = analyzer.detect_suspicious_clusters(&transactions).unwrap();
        
        assert!(!clusters.is_empty());
        assert!(clusters[0].transaction_count >= min_bundle_size);
    }
}
