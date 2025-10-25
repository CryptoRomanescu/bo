//! Bot and Bundle Detector
//!
//! Main detector that combines bot activity analysis and bundle detection

use super::analyzer::BotActivityAnalyzer;
use super::types::*;
use super::{BotBundleConfig, TokenClassification};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signature::Signature;
use solana_transaction_status::UiTransactionEncoding;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;
use tokio::time::timeout;
use tracing::{debug, info, instrument, warn};

/// Main bot and bundle detector
pub struct BotBundleDetector {
    config: BotBundleConfig,
    pub(crate) analyzer: BotActivityAnalyzer,
    rpc_client: Arc<RpcClient>,
}

impl BotBundleDetector {
    /// Create a new bot and bundle detector
    pub fn new(config: BotBundleConfig, rpc_client: Arc<RpcClient>) -> Self {
        let analyzer = BotActivityAnalyzer::new(config.clone());
        Self {
            config,
            analyzer,
            rpc_client,
        }
    }

    /// Analyze transactions manually (useful for testing/demos)
    /// This is a public helper that allows analyzing pre-fetched transaction data
    pub fn analyze_transactions(
        &self,
        transactions: Vec<TransactionTiming>,
        token_mint: &str,
        deploy_timestamp: u64,
    ) -> Result<BotBundleAnalysis> {
        let start_time = Instant::now();
        
        if transactions.len() < self.config.min_transactions {
            warn!(
                "Insufficient transactions for analysis: {} < {}",
                transactions.len(),
                self.config.min_transactions
            );
        }

        // Analyze bot activity
        let bot_metrics = self
            .analyzer
            .analyze_bot_activity(&transactions, deploy_timestamp)?;

        // Detect bundles
        let bundle_metrics = self.detect_bundles(&transactions)?;

        // Detect suspicious clusters
        let suspicious_clusters = self.analyzer.detect_suspicious_clusters(&transactions)?;

        // Detect repeated addresses
        let repeated_addresses = self.analyzer.detect_repeated_addresses(&transactions)?;

        // Calculate organic ratio
        let organic_ratio = self
            .analyzer
            .calculate_organic_ratio(&bot_metrics, transactions.len());

        let bot_percentage = if transactions.is_empty() {
            0.0
        } else {
            bot_metrics.bot_transaction_count as f64 / transactions.len() as f64
        };

        // Calculate manipulation score
        let manipulation_score =
            self.calculate_manipulation_score(&bot_metrics, &bundle_metrics, &suspicious_clusters);

        // Classify token
        let classification =
            self.classify_token(bot_percentage, organic_ratio, &bundle_metrics, &suspicious_clusters);

        let analysis_duration_ms = start_time.elapsed().as_millis() as u64;

        Ok(BotBundleAnalysis {
            mint: token_mint.to_string(),
            deploy_timestamp,
            analysis_timestamp: Utc::now().timestamp() as u64,
            total_transactions: transactions.len(),
            bot_metrics,
            bundle_metrics,
            organic_ratio,
            bot_percentage,
            classification,
            suspicious_clusters,
            repeated_addresses,
            manipulation_score,
            analysis_duration_ms,
        })
    }

    /// Analyze a token for bot activity and bundling
    #[instrument(skip(self), fields(token_mint))]
    pub async fn analyze_token(
        &self,
        token_mint: &str,
        deploy_timestamp: u64,
    ) -> Result<BotBundleAnalysis> {
        let start_time = Instant::now();
        info!("Starting bot/bundle analysis for token: {}", token_mint);

        // Fetch transactions with timeout
        let transactions = timeout(
            self.config.analysis_timeout,
            self.fetch_token_transactions(token_mint, deploy_timestamp),
        )
        .await
        .context("Analysis timeout")?
        .context("Failed to fetch transactions")?;

        if transactions.len() < self.config.min_transactions {
            warn!(
                "Insufficient transactions for analysis: {} < {}",
                transactions.len(),
                self.config.min_transactions
            );
        }

        // Analyze bot activity
        let bot_metrics = self
            .analyzer
            .analyze_bot_activity(&transactions, deploy_timestamp)?;

        // Detect bundles
        let bundle_metrics = self.detect_bundles(&transactions)?;

        // Detect suspicious clusters
        let suspicious_clusters = self.analyzer.detect_suspicious_clusters(&transactions)?;

        // Detect repeated addresses
        let repeated_addresses = self.analyzer.detect_repeated_addresses(&transactions)?;

        // Calculate organic ratio
        let organic_ratio = self
            .analyzer
            .calculate_organic_ratio(&bot_metrics, transactions.len());

        let bot_percentage = if transactions.is_empty() {
            0.0
        } else {
            bot_metrics.bot_transaction_count as f64 / transactions.len() as f64
        };

        // Calculate manipulation score
        let manipulation_score =
            self.calculate_manipulation_score(&bot_metrics, &bundle_metrics, &suspicious_clusters);

        // Classify token
        let classification =
            self.classify_token(bot_percentage, organic_ratio, &bundle_metrics, &suspicious_clusters);

        let analysis_duration_ms = start_time.elapsed().as_millis() as u64;

        let analysis = BotBundleAnalysis {
            mint: token_mint.to_string(),
            deploy_timestamp,
            analysis_timestamp: Utc::now().timestamp() as u64,
            total_transactions: transactions.len(),
            bot_metrics,
            bundle_metrics,
            organic_ratio,
            bot_percentage,
            classification,
            suspicious_clusters,
            repeated_addresses,
            manipulation_score,
            analysis_duration_ms,
        };

        info!(
            "Analysis complete in {}ms: {} (bot: {:.1}%, organic: {:.1}%)",
            analysis_duration_ms,
            classification.description(),
            bot_percentage * 100.0,
            organic_ratio * 100.0
        );

        Ok(analysis)
    }

    /// Fetch token transactions from Solana
    /// 
    /// TODO: This is a placeholder for full RPC integration
    /// In production, this would:
    /// 1. Use rpc_client.get_signatures_for_address()
    /// 2. Filter by time window
    /// 3. Fetch transaction details for each signature
    /// 4. Parse and extract timing data
    /// 5. Create TransactionTiming objects
    #[instrument(skip(self))]
    async fn fetch_token_transactions(
        &self,
        token_mint: &str,
        deploy_timestamp: u64,
    ) -> Result<Vec<TransactionTiming>> {
        let transactions = Vec::new();

        // Use Solana RPC to fetch token transaction signatures
        // Limited to the analysis window
        let analysis_window_secs = self.config.analysis_window_secs as i64;
        let max_tx_count = self.config.max_transactions;

        debug!(
            "Fetching transactions for token {} within {}s window (stub implementation)",
            token_mint, analysis_window_secs
        );

        // For integration with actual RPC, see graph_analyzer for reference implementation
        // This stub allows the detector to be instantiated and tested with manual data
        
        Ok(transactions)
    }

    /// Detect transaction bundles
    #[instrument(skip(self, transactions))]
    pub(crate) fn detect_bundles(&self, transactions: &[TransactionTiming]) -> Result<BundleMetrics> {
        if transactions.is_empty() {
            return Ok(BundleMetrics {
                bundle_count: 0,
                bundled_transaction_count: 0,
                bundle_percentage: 0.0,
                max_bundle_size: 0,
                avg_bundle_size: 0.0,
                coordinated_bundle_count: 0,
            });
        }

        let bundles = self.find_bundles(transactions)?;

        let bundle_count = bundles.len();
        let bundled_transaction_count: usize = bundles.iter().map(|b| b.transactions.len()).sum();
        let bundle_percentage = bundled_transaction_count as f64 / transactions.len() as f64;
        let max_bundle_size = bundles.iter().map(|b| b.transactions.len()).max().unwrap_or(0);
        let avg_bundle_size = if bundle_count > 0 {
            bundled_transaction_count as f64 / bundle_count as f64
        } else {
            0.0
        };
        let coordinated_bundle_count = bundles.iter().filter(|b| b.is_coordinated).count();

        debug!(
            "Detected {} bundles, {} coordinated",
            bundle_count, coordinated_bundle_count
        );

        Ok(BundleMetrics {
            bundle_count,
            bundled_transaction_count,
            bundle_percentage,
            max_bundle_size,
            avg_bundle_size,
            coordinated_bundle_count,
        })
    }

    /// Find bundles in transactions
    fn find_bundles(&self, transactions: &[TransactionTiming]) -> Result<Vec<TransactionBundle>> {
        let mut bundles = Vec::new();

        if transactions.len() < self.config.min_bundle_size {
            return Ok(bundles);
        }

        // Sort by timestamp
        let mut sorted = transactions.to_vec();
        sorted.sort_by_key(|tx| tx.timestamp);

        let mut bundle_id = 0;
        let mut i = 0;

        while i < sorted.len() {
            let start = &sorted[i];
            let mut bundle_txs = vec![start.clone()];
            let mut addresses = std::collections::HashSet::new();
            addresses.insert(start.address.clone());

            // Find transactions within bundle time window
            let mut j = i + 1;
            while j < sorted.len() {
                let current = &sorted[j];
                let time_diff = (current.timestamp - start.timestamp).num_milliseconds() as u64;

                if time_diff <= self.config.bundle_time_window_ms {
                    bundle_txs.push(current.clone());
                    addresses.insert(current.address.clone());
                    j += 1;
                } else {
                    break;
                }
            }

            // If bundle is large enough, record it
            if bundle_txs.len() >= self.config.min_bundle_size {
                let time_window_ms = (bundle_txs.last().unwrap().timestamp
                    - bundle_txs.first().unwrap().timestamp)
                    .num_milliseconds() as u64;

                let is_coordinated = addresses.len() > 1;

                bundles.push(TransactionBundle {
                    id: bundle_id,
                    transactions: bundle_txs,
                    unique_addresses: addresses.into_iter().collect(),
                    time_window_ms,
                    is_coordinated,
                });

                bundle_id += 1;
                i = j; // Skip to next potential bundle
            } else {
                i += 1;
            }
        }

        Ok(bundles)
    }

    /// Calculate overall manipulation score
    pub(crate) fn calculate_manipulation_score(
        &self,
        bot_metrics: &BotMetrics,
        bundle_metrics: &BundleMetrics,
        suspicious_clusters: &[SuspiciousCluster],
    ) -> f64 {
        let bot_score = bot_metrics.instant_transaction_percentage;
        let bundle_score = bundle_metrics.bundle_percentage;
        let cluster_score = if suspicious_clusters.is_empty() {
            0.0
        } else {
            suspicious_clusters
                .iter()
                .map(|c| c.suspicion_score)
                .sum::<f64>()
                / suspicious_clusters.len() as f64
        };

        // Weighted combination
        (bot_score * 0.5 + bundle_score * 0.3 + cluster_score * 0.2).min(1.0)
    }

    /// Classify token based on analysis
    pub(crate) fn classify_token(
        &self,
        bot_percentage: f64,
        organic_ratio: f64,
        bundle_metrics: &BundleMetrics,
        suspicious_clusters: &[SuspiciousCluster],
    ) -> TokenClassification {
        // Check for coordinated bundles first
        if bundle_metrics.coordinated_bundle_count > 3
            && bundle_metrics.bundle_percentage > 0.5
        {
            return TokenClassification::BundleCoordinated;
        }

        // Check for highly manipulated
        if bot_percentage >= self.config.avoid_bot_threshold {
            return TokenClassification::HighlyManipulated;
        }

        // Check for bot-dominated
        if organic_ratio < self.config.healthy_organic_threshold {
            return TokenClassification::BotDominated;
        }

        // Check for organic
        if organic_ratio > 0.7 {
            return TokenClassification::Organic;
        }

        // Default to mixed
        TokenClassification::Mixed
    }

    /// Get scoring result for decision engine integration
    pub fn get_score(&self, analysis: &BotBundleAnalysis) -> BotBundleScore {
        BotBundleScore::from_analysis(analysis)
    }

    /// Get configuration
    pub fn config(&self) -> &BotBundleConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_rpc_client() -> Arc<RpcClient> {
        Arc::new(RpcClient::new(
            "https://api.mainnet-beta.solana.com".to_string(),
        ))
    }

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
    fn test_detector_creation() {
        let config = BotBundleConfig::default();
        let rpc_client = create_test_rpc_client();
        let _detector = BotBundleDetector::new(config, rpc_client);
    }

    #[test]
    fn test_bundle_detection() {
        let config = BotBundleConfig::default();
        let rpc_client = create_test_rpc_client();
        let detector = BotBundleDetector::new(config, rpc_client);

        let base_time = Utc::now();
        let transactions = vec![
            create_test_transaction("addr1", base_time, 50),
            create_test_transaction("addr2", base_time + chrono::Duration::milliseconds(100), 150),
            create_test_transaction("addr3", base_time + chrono::Duration::milliseconds(200), 250),
            create_test_transaction("addr4", base_time + chrono::Duration::milliseconds(300), 350),
        ];

        let bundles = detector.find_bundles(&transactions).unwrap();
        assert!(!bundles.is_empty());
        assert!(bundles[0].is_coordinated);
    }

    #[test]
    fn test_token_classification() {
        let config = BotBundleConfig::default();
        let rpc_client = create_test_rpc_client();
        let detector = BotBundleDetector::new(config, rpc_client);

        let bundle_metrics = BundleMetrics {
            bundle_count: 5,
            bundled_transaction_count: 15,
            bundle_percentage: 0.15,
            max_bundle_size: 5,
            avg_bundle_size: 3.0,
            coordinated_bundle_count: 2,
        };

        // Test organic
        let classification =
            detector.classify_token(0.2, 0.8, &bundle_metrics, &[]);
        assert_eq!(classification, TokenClassification::Organic);

        // Test bot-dominated
        let classification =
            detector.classify_token(0.75, 0.25, &bundle_metrics, &[]);
        assert_eq!(classification, TokenClassification::BotDominated);

        // Test highly manipulated
        let classification =
            detector.classify_token(0.90, 0.10, &bundle_metrics, &[]);
        assert_eq!(classification, TokenClassification::HighlyManipulated);
    }

    #[test]
    fn test_manipulation_score_calculation() {
        let config = BotBundleConfig::default();
        let rpc_client = create_test_rpc_client();
        let detector = BotBundleDetector::new(config, rpc_client);

        let bot_metrics = BotMetrics {
            bot_transaction_count: 85,
            unique_bot_addresses: 20,
            avg_bot_response_time_ms: 50.0,
            instant_transaction_percentage: 0.85,
            high_frequency_bots: 5,
        };

        let bundle_metrics = BundleMetrics {
            bundle_count: 10,
            bundled_transaction_count: 30,
            bundle_percentage: 0.30,
            max_bundle_size: 5,
            avg_bundle_size: 3.0,
            coordinated_bundle_count: 8,
        };

        let score = detector.calculate_manipulation_score(&bot_metrics, &bundle_metrics, &[]);
        assert!(score > 0.5);
        assert!(score <= 1.0);
    }
}
