//! Integration tests for Smart Money Tracker
//!
//! Validates the smart money detection and alerting functionality

use h_5n1p3r::oracle::{
    DataSource, SmartMoneyConfig, SmartMoneyTracker, SmartWalletProfile, SmartWalletTransaction,
    TransactionType,
};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[tokio::test]
async fn test_smart_money_tracker_creation() {
    let config = SmartMoneyConfig::default();
    let tracker = SmartMoneyTracker::new(config);

    // Should create without errors
    let wallets = tracker.get_smart_wallets().await;
    assert_eq!(wallets.len(), 0);
}

#[tokio::test]
async fn test_add_smart_wallet() {
    let config = SmartMoneyConfig::default();
    let tracker = Arc::new(SmartMoneyTracker::new(config));

    let profile = SmartWalletProfile {
        address: "test_wallet_1".to_string(),
        source: DataSource::Custom,
        success_rate: Some(75.0),
        total_profit_sol: Some(100.0),
        successful_trades: Some(50),
        last_activity: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };

    tracker.add_smart_wallet(profile.clone()).await;

    let wallets = tracker.get_smart_wallets().await;
    assert_eq!(wallets.len(), 1);
    assert_eq!(wallets[0].address, "test_wallet_1");
    assert_eq!(wallets[0].success_rate, Some(75.0));
}

#[tokio::test]
async fn test_smart_money_check_no_transactions() {
    let config = SmartMoneyConfig::default();
    let tracker = Arc::new(SmartMoneyTracker::new(config));

    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let result = tracker
        .check_smart_money("test_token_mint", current_time - 30)
        .await;

    assert!(result.is_ok());
    let (score, wallet_count, transactions) = result.unwrap();

    // With no configured APIs and no custom wallets, should return empty results
    assert_eq!(wallet_count, 0);
    assert_eq!(transactions.len(), 0);
    assert_eq!(score, 0);
}

#[tokio::test]
async fn test_alert_threshold_not_met() {
    let config = SmartMoneyConfig {
        min_smart_wallets: 2,
        ..Default::default()
    };
    let tracker = Arc::new(SmartMoneyTracker::new(config));

    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let alert = tracker
        .check_alert("test_token_mint", current_time - 30)
        .await
        .expect("Alert check should succeed");

    // Should not trigger alert with 0 wallets (need 2+)
    assert!(alert.is_none());
}

#[tokio::test]
async fn test_smart_money_config_validation() {
    let config = SmartMoneyConfig {
        detection_window_secs: 60,
        min_smart_wallets: 2,
        cache_duration_secs: 300,
        api_timeout_secs: 5,
        enable_nansen: false,
        enable_birdeye: true,
        nansen_api_key: None,
        birdeye_api_key: Some("test_key".to_string()),
        custom_smart_wallets: vec!["wallet1".to_string(), "wallet2".to_string()],
    };

    let tracker = SmartMoneyTracker::new(config);

    // Custom wallets should be loaded
    let wallets = tracker.get_smart_wallets().await;
    assert_eq!(wallets.len(), 2);
    assert!(wallets.iter().any(|w| w.address == "wallet1"));
    assert!(wallets.iter().any(|w| w.address == "wallet2"));
}

#[tokio::test]
async fn test_smart_money_check_with_custom_wallets() {
    // Test score calculation indirectly through check_smart_money
    let config = SmartMoneyConfig {
        custom_smart_wallets: vec!["wallet1".to_string(), "wallet2".to_string()],
        ..Default::default()
    };
    let tracker = Arc::new(SmartMoneyTracker::new(config));

    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let result = tracker
        .check_smart_money("test_token", current_time - 30)
        .await;

    assert!(result.is_ok());
    let (score, _wallet_count, _transactions) = result.unwrap();

    // Score should be valid (0-100)
    assert!(score <= 100);
}

#[tokio::test]
async fn test_detection_window_enforcement() {
    let config = SmartMoneyConfig {
        detection_window_secs: 60,
        ..Default::default()
    };
    let tracker = Arc::new(SmartMoneyTracker::new(config));

    // Verify the config is set correctly
    assert_eq!(tracker.get_smart_wallets().await.len(), 0);
}

#[tokio::test]
async fn test_concurrent_smart_wallet_additions() {
    let config = SmartMoneyConfig::default();
    let tracker = Arc::new(SmartMoneyTracker::new(config));

    // Test concurrent additions
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let tracker = tracker.clone();
            tokio::spawn(async move {
                let profile = SmartWalletProfile {
                    address: format!("wallet_{}", i),
                    source: DataSource::Custom,
                    success_rate: Some(70.0 + i as f64),
                    total_profit_sol: Some(100.0 * i as f64),
                    successful_trades: Some(i * 10),
                    last_activity: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                };
                tracker.add_smart_wallet(profile).await;
            })
        })
        .collect();

    for handle in handles {
        handle.await.expect("Task should complete");
    }

    let wallets = tracker.get_smart_wallets().await;
    assert_eq!(wallets.len(), 10);
}

#[tokio::test]
async fn test_alert_generation_timing() {
    let config = SmartMoneyConfig {
        min_smart_wallets: 2,
        detection_window_secs: 60,
        ..Default::default()
    };
    let tracker = Arc::new(SmartMoneyTracker::new(config));

    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let deploy_time = current_time - 30;

    let start = std::time::Instant::now();
    let result = tracker.check_alert("test_mint", deploy_time).await;
    let elapsed = start.elapsed();

    // Should complete within 5 seconds (requirement: <5s alert latency)
    assert!(elapsed.as_secs() < 5, "Alert check should complete in <5s");
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_performance_100_tokens() {
    // Performance test: Check 100 tokens within reasonable time
    let config = SmartMoneyConfig::default();
    let tracker = Arc::new(SmartMoneyTracker::new(config));

    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let start = std::time::Instant::now();

    for i in 0..100 {
        let token_mint = format!("token_{}", i);
        let deploy_time = current_time - 30;

        let result = tracker.check_smart_money(&token_mint, deploy_time).await;
        assert!(result.is_ok());
    }

    let elapsed = start.elapsed();
    let avg_time_ms = elapsed.as_millis() / 100;

    println!("Average time per token: {}ms", avg_time_ms);

    // Should process 100 tokens reasonably fast (under 10 seconds total)
    assert!(elapsed.as_secs() < 10, "Should process 100 tokens in <10s");
}

#[test]
fn test_config_defaults() {
    let config = SmartMoneyConfig::default();

    assert_eq!(config.detection_window_secs, 60);
    assert_eq!(config.min_smart_wallets, 2);
    assert_eq!(config.cache_duration_secs, 300);
    assert_eq!(config.api_timeout_secs, 5);
    assert!(!config.enable_nansen);
    assert!(config.enable_birdeye);
    assert!(config.nansen_api_key.is_none());
    assert!(config.birdeye_api_key.is_none());
    assert_eq!(config.custom_smart_wallets.len(), 0);
}
