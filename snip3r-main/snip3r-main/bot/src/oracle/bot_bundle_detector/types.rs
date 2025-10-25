//! Types for bot and bundle detection

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result of bot and bundle analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotBundleAnalysis {
    /// Token mint address
    pub mint: String,
    
    /// Token deployment timestamp
    pub deploy_timestamp: u64,
    
    /// Analysis timestamp
    pub analysis_timestamp: u64,
    
    /// Total transactions analyzed
    pub total_transactions: usize,
    
    /// Bot activity metrics
    pub bot_metrics: BotMetrics,
    
    /// Bundle detection metrics
    pub bundle_metrics: BundleMetrics,
    
    /// Organic vs robotic ratio
    pub organic_ratio: f64,
    
    /// Bot percentage (0.0-1.0)
    pub bot_percentage: f64,
    
    /// Classification based on analysis
    pub classification: super::TokenClassification,
    
    /// Suspicious clusters detected
    pub suspicious_clusters: Vec<SuspiciousCluster>,
    
    /// Repeated addresses (high-frequency traders)
    pub repeated_addresses: Vec<RepeatedAddress>,
    
    /// Overall manipulation score (0.0-1.0)
    pub manipulation_score: f64,
    
    /// Analysis duration in milliseconds
    pub analysis_duration_ms: u64,
}

/// Bot activity metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotMetrics {
    /// Number of transactions classified as bot activity
    pub bot_transaction_count: usize,
    
    /// Number of unique bot addresses detected
    pub unique_bot_addresses: usize,
    
    /// Average time between deploy and bot transactions (ms)
    pub avg_bot_response_time_ms: f64,
    
    /// Percentage of transactions within bot timing threshold
    pub instant_transaction_percentage: f64,
    
    /// High-frequency bot addresses
    pub high_frequency_bots: usize,
}

/// Bundle detection metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleMetrics {
    /// Number of bundles detected
    pub bundle_count: usize,
    
    /// Total transactions in bundles
    pub bundled_transaction_count: usize,
    
    /// Percentage of transactions that are bundled
    pub bundle_percentage: f64,
    
    /// Largest bundle size
    pub max_bundle_size: usize,
    
    /// Average bundle size
    pub avg_bundle_size: f64,
    
    /// Coordinated bundles (multiple addresses in tight timing)
    pub coordinated_bundle_count: usize,
}

/// Suspicious cluster of related transactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuspiciousCluster {
    /// Cluster ID
    pub id: usize,
    
    /// Addresses involved in cluster
    pub addresses: Vec<String>,
    
    /// Number of transactions in cluster
    pub transaction_count: usize,
    
    /// Time window of cluster (milliseconds)
    pub time_window_ms: u64,
    
    /// Cluster suspicion score (0.0-1.0)
    pub suspicion_score: f64,
    
    /// Reason for flagging
    pub reason: String,
}

/// Repeated address (potential bot)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepeatedAddress {
    /// Wallet address
    pub address: String,
    
    /// Number of transactions from this address
    pub transaction_count: usize,
    
    /// Transactions per second
    pub transactions_per_second: f64,
    
    /// Average time between transactions (ms)
    pub avg_time_between_txs_ms: f64,
    
    /// First transaction timestamp
    pub first_transaction: DateTime<Utc>,
    
    /// Last transaction timestamp
    pub last_transaction: DateTime<Utc>,
    
    /// Bot probability (0.0-1.0)
    pub bot_probability: f64,
}

/// Transaction timing data for analysis
#[derive(Debug, Clone)]
pub struct TransactionTiming {
    /// Transaction signature
    pub signature: String,
    
    /// Wallet address
    pub address: String,
    
    /// Transaction timestamp
    pub timestamp: DateTime<Utc>,
    
    /// Time since token deployment (milliseconds)
    pub time_since_deploy_ms: u64,
    
    /// Transaction slot
    pub slot: u64,
    
    /// Whether part of a bundle
    pub in_bundle: bool,
    
    /// Bundle ID if in bundle
    pub bundle_id: Option<usize>,
}

/// Bundle of coordinated transactions
#[derive(Debug, Clone)]
pub struct TransactionBundle {
    /// Bundle ID
    pub id: usize,
    
    /// Transactions in bundle
    pub transactions: Vec<TransactionTiming>,
    
    /// Unique addresses in bundle
    pub unique_addresses: Vec<String>,
    
    /// Time window of bundle (milliseconds)
    pub time_window_ms: u64,
    
    /// Whether bundle is coordinated (multiple unique addresses)
    pub is_coordinated: bool,
}

/// Scoring result for decision engine integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotBundleScore {
    /// Bot penalty score (0-100, higher = worse)
    pub bot_penalty: u8,
    
    /// Bundle penalty score (0-100, higher = worse)
    pub bundle_penalty: u8,
    
    /// Organic bonus score (0-100, higher = better)
    pub organic_bonus: u8,
    
    /// Combined manipulation score (0-100, higher = worse)
    pub manipulation_score: u8,
    
    /// Should this token be avoided
    pub should_avoid: bool,
    
    /// Confidence in scoring (0.0-1.0)
    pub confidence: f64,
    
    /// Human-readable explanation
    pub explanation: String,
}

impl BotBundleScore {
    /// Create a new score with default values
    pub fn new() -> Self {
        Self {
            bot_penalty: 0,
            bundle_penalty: 0,
            organic_bonus: 0,
            manipulation_score: 0,
            should_avoid: false,
            confidence: 0.0,
            explanation: String::new(),
        }
    }
    
    /// Calculate from analysis result
    pub fn from_analysis(analysis: &BotBundleAnalysis) -> Self {
        let bot_penalty = (analysis.bot_percentage * 100.0) as u8;
        let bundle_penalty = (analysis.bundle_metrics.bundle_percentage * 100.0) as u8;
        let organic_bonus = (analysis.organic_ratio * 100.0) as u8;
        let manipulation_score = (analysis.manipulation_score * 100.0) as u8;
        let should_avoid = analysis.classification.should_avoid();
        
        // Confidence based on number of transactions analyzed
        let confidence = if analysis.total_transactions < 20 {
            0.3
        } else if analysis.total_transactions < 50 {
            0.6
        } else {
            0.9
        };
        
        let explanation = format!(
            "{} - Bot: {:.1}%, Organic: {:.1}%, Bundles: {}",
            analysis.classification.description(),
            analysis.bot_percentage * 100.0,
            analysis.organic_ratio * 100.0,
            analysis.bundle_metrics.bundle_count
        );
        
        Self {
            bot_penalty,
            bundle_penalty,
            organic_bonus,
            manipulation_score,
            should_avoid,
            confidence,
            explanation,
        }
    }
}

impl Default for BotBundleScore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::bot_bundle_detector::TokenClassification;

    #[test]
    fn test_bot_bundle_score_from_analysis() {
        let analysis = BotBundleAnalysis {
            mint: "test_mint".to_string(),
            deploy_timestamp: 1000,
            analysis_timestamp: 2000,
            total_transactions: 100,
            bot_metrics: BotMetrics {
                bot_transaction_count: 85,
                unique_bot_addresses: 20,
                avg_bot_response_time_ms: 50.0,
                instant_transaction_percentage: 0.85,
                high_frequency_bots: 5,
            },
            bundle_metrics: BundleMetrics {
                bundle_count: 10,
                bundled_transaction_count: 30,
                bundle_percentage: 0.30,
                max_bundle_size: 5,
                avg_bundle_size: 3.0,
                coordinated_bundle_count: 8,
            },
            organic_ratio: 0.15,
            bot_percentage: 0.85,
            classification: TokenClassification::HighlyManipulated,
            suspicious_clusters: vec![],
            repeated_addresses: vec![],
            manipulation_score: 0.9,
            analysis_duration_ms: 5000,
        };
        
        let score = BotBundleScore::from_analysis(&analysis);
        assert_eq!(score.bot_penalty, 85);
        assert_eq!(score.bundle_penalty, 30);
        assert_eq!(score.organic_bonus, 15);
        assert_eq!(score.manipulation_score, 90);
        assert!(score.should_avoid);
        assert!(score.confidence > 0.5);
    }

    #[test]
    fn test_default_bot_bundle_score() {
        let score = BotBundleScore::default();
        assert_eq!(score.bot_penalty, 0);
        assert_eq!(score.manipulation_score, 0);
        assert!(!score.should_avoid);
    }
}
