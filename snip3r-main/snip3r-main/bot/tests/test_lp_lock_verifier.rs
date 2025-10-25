//! Integration tests for LP Lock Verifier
//!
//! Validates LP lock/burn detection and risk assessment

use h_5n1p3r::oracle::{
    LockStatus, LpLockConfig, LpLockVerifier, RiskLevel,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use std::time::Instant;

#[tokio::test]
async fn test_lp_verification_performance() {
    // Test that verification completes within 5s requirement
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
    let verifier = LpLockVerifier::new(config, rpc);

    let start = Instant::now();
    
    // Verify a test token (this will likely fail with Unknown status in test)
    let result = verifier
        .verify("TestMintAddress123456789", "pump.fun")
        .await;

    let elapsed = start.elapsed().as_millis();

    // Should complete within 5 seconds
    assert!(
        elapsed < 5000,
        "LP verification took {}ms, exceeds 5s requirement",
        elapsed
    );

    // Result should be Ok or expected error
    match result {
        Ok(verification) => {
            println!("Verification completed in {}ms", elapsed);
            println!("Status: {:?}", verification.lock_status);
            println!("Risk: {:?}", verification.risk_level);
            println!("Safety score: {}", verification.safety_score);
            println!("Auto reject: {}", verification.auto_reject);
        }
        Err(e) => {
            // In test environment, RPC errors are expected
            println!("Verification error (expected in test): {}", e);
        }
    }
}

#[test]
fn test_risk_level_locked() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test fully locked with 1 year duration - should be Minimal risk
    let locked_full = LockStatus::Locked {
        contract: "LocktDzaV1W2Bm9DeZeiyz4J9zs4fRqNiYqQyracRXw".to_string(),
        duration_secs: 365 * 24 * 60 * 60, // 1 year
        expiry_timestamp: chrono::Utc::now().timestamp() as u64 + 365 * 24 * 60 * 60,
        percentage_locked: 100,
    };
    assert_eq!(verifier.calculate_risk_level(&locked_full), RiskLevel::Minimal);

    // Test 80% locked with short duration - should be Low risk
    let locked_80 = LockStatus::Locked {
        contract: "LocktDzaV1W2Bm9DeZeiyz4J9zs4fRqNiYqQyracRXw".to_string(),
        duration_secs: 90 * 24 * 60 * 60, // 90 days
        expiry_timestamp: chrono::Utc::now().timestamp() as u64 + 90 * 24 * 60 * 60,
        percentage_locked: 80,
    };
    assert_eq!(verifier.calculate_risk_level(&locked_80), RiskLevel::Low);

    // Test 50% locked - should be Medium risk
    let locked_50 = LockStatus::Locked {
        contract: "LocktDzaV1W2Bm9DeZeiyz4J9zs4fRqNiYqQyracRXw".to_string(),
        duration_secs: 180 * 24 * 60 * 60, // 180 days
        expiry_timestamp: chrono::Utc::now().timestamp() as u64 + 180 * 24 * 60 * 60,
        percentage_locked: 50,
    };
    assert_eq!(verifier.calculate_risk_level(&locked_50), RiskLevel::Medium);

    // Test 30% locked - should be High risk
    let locked_30 = LockStatus::Locked {
        contract: "LocktDzaV1W2Bm9DeZeiyz4J9zs4fRqNiYqQyracRXw".to_string(),
        duration_secs: 180 * 24 * 60 * 60,
        expiry_timestamp: chrono::Utc::now().timestamp() as u64 + 180 * 24 * 60 * 60,
        percentage_locked: 30,
    };
    assert_eq!(verifier.calculate_risk_level(&locked_30), RiskLevel::High);
}

#[test]
fn test_risk_level_burned() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test fully burned - should be Minimal risk
    let burned_full = LockStatus::Burned {
        burn_address: "11111111111111111111111111111111".to_string(),
        percentage_burned: 100,
    };
    assert_eq!(verifier.calculate_risk_level(&burned_full), RiskLevel::Minimal);

    // Test 85% burned - should be Low risk
    let burned_85 = LockStatus::Burned {
        burn_address: "11111111111111111111111111111111".to_string(),
        percentage_burned: 85,
    };
    assert_eq!(verifier.calculate_risk_level(&burned_85), RiskLevel::Low);

    // Test 60% burned - should be Medium risk
    let burned_60 = LockStatus::Burned {
        burn_address: "11111111111111111111111111111111".to_string(),
        percentage_burned: 60,
    };
    assert_eq!(verifier.calculate_risk_level(&burned_60), RiskLevel::Medium);

    // Test 40% burned - should be High risk
    let burned_40 = LockStatus::Burned {
        burn_address: "11111111111111111111111111111111".to_string(),
        percentage_burned: 40,
    };
    assert_eq!(verifier.calculate_risk_level(&burned_40), RiskLevel::High);
}

#[test]
fn test_risk_level_unlocked() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test fully unlocked - should be Critical risk
    let unlocked = LockStatus::Unlocked {
        percentage_unlocked: 100,
    };
    assert_eq!(verifier.calculate_risk_level(&unlocked), RiskLevel::Critical);

    // Test unknown - should be Critical risk
    let unknown = LockStatus::Unknown {
        reason: "Unable to verify".to_string(),
    };
    assert_eq!(verifier.calculate_risk_level(&unknown), RiskLevel::Critical);
}

#[test]
fn test_risk_level_partial() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test 50% locked + 50% burned = 100% secured - should be Minimal risk
    let partial_full = LockStatus::Partial {
        locked_percentage: 50,
        burned_percentage: 50,
        lock_info: None,
    };
    assert_eq!(verifier.calculate_risk_level(&partial_full), RiskLevel::Minimal);

    // Test 40% locked + 45% burned = 85% secured - should be Low risk
    let partial_85 = LockStatus::Partial {
        locked_percentage: 40,
        burned_percentage: 45,
        lock_info: None,
    };
    assert_eq!(verifier.calculate_risk_level(&partial_85), RiskLevel::Low);

    // Test 30% locked + 30% burned = 60% secured - should be Medium risk
    let partial_60 = LockStatus::Partial {
        locked_percentage: 30,
        burned_percentage: 30,
        lock_info: None,
    };
    assert_eq!(verifier.calculate_risk_level(&partial_60), RiskLevel::Medium);

    // Test 20% locked + 20% burned = 40% secured - should be High risk
    let partial_40 = LockStatus::Partial {
        locked_percentage: 20,
        burned_percentage: 20,
        lock_info: None,
    };
    assert_eq!(verifier.calculate_risk_level(&partial_40), RiskLevel::High);
}

#[test]
fn test_safety_score_calculation() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test fully locked with 1 year - should be 100
    let locked_full = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 365 * 24 * 60 * 60,
        expiry_timestamp: 0,
        percentage_locked: 100,
    };
    assert_eq!(verifier.calculate_safety_score(&locked_full), 100);

    // Test 50% locked with 6 months - should be ~50
    let locked_half = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 180 * 24 * 60 * 60,
        expiry_timestamp: 0,
        percentage_locked: 50,
    };
    let score = verifier.calculate_safety_score(&locked_half);
    assert!(score >= 45 && score <= 55, "Expected ~50, got {}", score);

    // Test fully burned - should be 100
    let burned_full = LockStatus::Burned {
        burn_address: "test".to_string(),
        percentage_burned: 100,
    };
    assert_eq!(verifier.calculate_safety_score(&burned_full), 100);

    // Test 50% burned - should be 50
    let burned_half = LockStatus::Burned {
        burn_address: "test".to_string(),
        percentage_burned: 50,
    };
    assert_eq!(verifier.calculate_safety_score(&burned_half), 50);

    // Test fully unlocked - should be 0
    let unlocked = LockStatus::Unlocked {
        percentage_unlocked: 100,
    };
    assert_eq!(verifier.calculate_safety_score(&unlocked), 0);

    // Test unknown - should be 0
    let unknown = LockStatus::Unknown {
        reason: "Test".to_string(),
    };
    assert_eq!(verifier.calculate_safety_score(&unknown), 0);
}

#[test]
fn test_auto_reject_logic() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
    let verifier = LpLockVerifier::new(config, rpc);

    // Critical risk should always auto-reject
    let unlocked = LockStatus::Unlocked {
        percentage_unlocked: 100,
    };
    let risk = verifier.calculate_risk_level(&unlocked);
    assert!(verifier.should_auto_reject(&unlocked, &risk));

    let unknown = LockStatus::Unknown {
        reason: "Test".to_string(),
    };
    let risk = verifier.calculate_risk_level(&unknown);
    assert!(verifier.should_auto_reject(&unknown, &risk));

    // Safe tokens should not auto-reject
    let locked = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 365 * 24 * 60 * 60,
        expiry_timestamp: 0,
        percentage_locked: 100,
    };
    let risk = verifier.calculate_risk_level(&locked);
    assert!(!verifier.should_auto_reject(&locked, &risk));

    let burned = LockStatus::Burned {
        burn_address: "test".to_string(),
        percentage_burned: 100,
    };
    let risk = verifier.calculate_risk_level(&burned);
    assert!(!verifier.should_auto_reject(&burned, &risk));
}

#[test]
fn test_notes_generation() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test locked status notes
    let locked = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 365 * 24 * 60 * 60,
        expiry_timestamp: 0,
        percentage_locked: 100,
    };
    let risk = verifier.calculate_risk_level(&locked);
    let notes = verifier.generate_notes(&locked, &risk);
    assert!(!notes.is_empty());
    assert!(notes.iter().any(|n| n.contains("100%")));
    assert!(notes.iter().any(|n| n.contains("365")));

    // Test burned status notes
    let burned = LockStatus::Burned {
        burn_address: "test".to_string(),
        percentage_burned: 100,
    };
    let risk = verifier.calculate_risk_level(&burned);
    let notes = verifier.generate_notes(&burned, &risk);
    assert!(!notes.is_empty());
    assert!(notes.iter().any(|n| n.contains("burned")));
    assert!(notes.iter().any(|n| n.contains("rug-pull impossible")));

    // Test unlocked status notes
    let unlocked = LockStatus::Unlocked {
        percentage_unlocked: 100,
    };
    let risk = verifier.calculate_risk_level(&unlocked);
    let notes = verifier.generate_notes(&unlocked, &risk);
    assert!(!notes.is_empty());
    assert!(notes.iter().any(|n| n.contains("CRITICAL")));
    assert!(notes.iter().any(|n| n.contains("RUG-PULL")));
}

#[test]
fn test_custom_config() {
    // Test with custom configuration
    let custom_config = LpLockConfig {
        timeout_secs: 3,
        min_lock_percentage: 90,
        min_lock_duration_days: 365,
        auto_reject_threshold: 60,
    };
    let rpc = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
    let verifier = LpLockVerifier::new(custom_config, rpc);

    // Test that custom thresholds are respected
    let locked_85 = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 200 * 24 * 60 * 60, // 200 days
        expiry_timestamp: 0,
        percentage_locked: 85,
    };
    let risk = verifier.calculate_risk_level(&locked_85);
    
    // With 90% threshold, 85% should be Medium risk
    assert!(risk >= RiskLevel::Medium);
}
