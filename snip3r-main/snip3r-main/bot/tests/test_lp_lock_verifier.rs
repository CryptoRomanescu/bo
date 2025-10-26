//! Integration tests for LP Lock Verifier
//!
//! Validates LP lock/burn detection and risk assessment

use h_5n1p3r::oracle::{LockStatus, LpLockConfig, LpLockVerifier, RiskLevel};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use std::time::Instant;

#[tokio::test]
async fn test_lp_verification_performance() {
    // Test that verification completes within 5s requirement
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
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
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test fully locked with 1 year duration - should be Minimal risk
    let locked_full = LockStatus::Locked {
        contract: "LocktDzaV1W2Bm9DeZeiyz4J9zs4fRqNiYqQyracRXw".to_string(),
        duration_secs: 365 * 24 * 60 * 60, // 1 year
        expiry_timestamp: chrono::Utc::now().timestamp() as u64 + 365 * 24 * 60 * 60,
        percentage_locked: 100,
    };
    assert_eq!(
        verifier.calculate_risk_level(&locked_full),
        RiskLevel::Minimal
    );

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
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test fully burned - should be Minimal risk
    let burned_full = LockStatus::Burned {
        burn_address: "11111111111111111111111111111111".to_string(),
        percentage_burned: 100,
    };
    assert_eq!(
        verifier.calculate_risk_level(&burned_full),
        RiskLevel::Minimal
    );

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
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test fully unlocked - should be Critical risk
    let unlocked = LockStatus::Unlocked {
        percentage_unlocked: 100,
    };
    assert_eq!(
        verifier.calculate_risk_level(&unlocked),
        RiskLevel::Critical
    );

    // Test unknown - should be Critical risk
    let unknown = LockStatus::Unknown {
        reason: "Unable to verify".to_string(),
    };
    assert_eq!(verifier.calculate_risk_level(&unknown), RiskLevel::Critical);
}

#[test]
fn test_risk_level_partial() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test 50% locked + 50% burned = 100% secured - should be Minimal risk
    let partial_full = LockStatus::Partial {
        locked_percentage: 50,
        burned_percentage: 50,
        lock_info: None,
    };
    assert_eq!(
        verifier.calculate_risk_level(&partial_full),
        RiskLevel::Minimal
    );

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
    assert_eq!(
        verifier.calculate_risk_level(&partial_60),
        RiskLevel::Medium
    );

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
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
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
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
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
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
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
        ..Default::default()
    };
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
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

#[test]
fn test_edge_case_zero_percentage() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test 0% locked - should be High risk
    let locked_0 = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 365 * 24 * 60 * 60,
        expiry_timestamp: 0,
        percentage_locked: 0,
    };
    assert_eq!(verifier.calculate_risk_level(&locked_0), RiskLevel::High);
    assert_eq!(verifier.calculate_safety_score(&locked_0), 20); // Only duration bonus

    // Test 0% burned - should be High risk
    let burned_0 = LockStatus::Burned {
        burn_address: "test".to_string(),
        percentage_burned: 0,
    };
    assert_eq!(verifier.calculate_risk_level(&burned_0), RiskLevel::High);
    assert_eq!(verifier.calculate_safety_score(&burned_0), 0);
}

#[test]
fn test_edge_case_soon_to_expire_lock() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test lock expiring in 1 day - should be Low or Medium risk
    let soon_expire = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 1 * 24 * 60 * 60, // 1 day
        expiry_timestamp: chrono::Utc::now().timestamp() as u64 + 24 * 60 * 60,
        percentage_locked: 100,
    };
    let risk = verifier.calculate_risk_level(&soon_expire);
    assert!(risk >= RiskLevel::Low); // Should not be minimal due to short duration

    let notes = verifier.generate_notes(&soon_expire, &risk);
    assert!(notes.iter().any(|n| n.contains("below recommended")));
}

#[test]
fn test_edge_case_partial_both_zero() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test 0% locked + 0% burned - should be High risk
    let partial_zero = LockStatus::Partial {
        locked_percentage: 0,
        burned_percentage: 0,
        lock_info: None,
    };
    assert_eq!(
        verifier.calculate_risk_level(&partial_zero),
        RiskLevel::High
    );
    assert_eq!(verifier.calculate_safety_score(&partial_zero), 0);
}

#[test]
fn test_edge_case_partial_exceeds_100() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test edge case where locked + burned might theoretically exceed 100%
    // (should be capped at 100)
    let partial_over = LockStatus::Partial {
        locked_percentage: 70,
        burned_percentage: 60,
        lock_info: None,
    };
    let score = verifier.calculate_safety_score(&partial_over);
    assert!(score <= 100, "Safety score should not exceed 100");
}

#[test]
fn test_auto_reject_high_risk_with_threshold() {
    let config = LpLockConfig {
        timeout_secs: 5,
        min_lock_percentage: 80,
        min_lock_duration_days: 180,
        auto_reject_threshold: 60, // Higher threshold
        ..Default::default()
    };
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Unlocked status is always Critical risk, so it will always auto-reject
    // regardless of percentage
    let unlocked_55 = LockStatus::Unlocked {
        percentage_unlocked: 55,
    };
    let risk = verifier.calculate_risk_level(&unlocked_55);
    assert_eq!(risk, RiskLevel::Critical);
    assert!(verifier.should_auto_reject(&unlocked_55, &risk));

    // Test with Partial status for threshold testing
    // 45% locked + 0% burned = 45% secured (below 60% threshold) - should auto-reject
    let partial_low = LockStatus::Partial {
        locked_percentage: 45,
        burned_percentage: 0,
        lock_info: None,
    };
    let risk = verifier.calculate_risk_level(&partial_low);
    assert!(verifier.should_auto_reject(&partial_low, &risk));

    // 65% locked + 0% burned = 65% secured (above 60% threshold) - should not auto-reject
    let partial_ok = LockStatus::Partial {
        locked_percentage: 65,
        burned_percentage: 0,
        lock_info: None,
    };
    let risk = verifier.calculate_risk_level(&partial_ok);
    assert!(!verifier.should_auto_reject(&partial_ok, &risk));
}

#[test]
fn test_partial_auto_reject_logic() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test partial with 20% locked + 20% burned = 40% secured (below 50% threshold)
    let partial_low = LockStatus::Partial {
        locked_percentage: 20,
        burned_percentage: 20,
        lock_info: None,
    };
    let risk = verifier.calculate_risk_level(&partial_low);
    assert!(verifier.should_auto_reject(&partial_low, &risk));

    // Test partial with 30% locked + 30% burned = 60% secured (above 50% threshold)
    let partial_ok = LockStatus::Partial {
        locked_percentage: 30,
        burned_percentage: 30,
        lock_info: None,
    };
    let risk = verifier.calculate_risk_level(&partial_ok);
    assert!(!verifier.should_auto_reject(&partial_ok, &risk));
}

#[test]
fn test_notes_all_status_types() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test Partial notes
    let partial = LockStatus::Partial {
        locked_percentage: 40,
        burned_percentage: 50,
        lock_info: None,
    };
    let risk = verifier.calculate_risk_level(&partial);
    let notes = verifier.generate_notes(&partial, &risk);
    assert!(notes.iter().any(|n| n.contains("Partial security")));
    assert!(notes.iter().any(|n| n.contains("40%") && n.contains("50%")));

    // Test Unknown notes
    let unknown = LockStatus::Unknown {
        reason: "RPC error".to_string(),
    };
    let risk = verifier.calculate_risk_level(&unknown);
    let notes = verifier.generate_notes(&unknown, &risk);
    assert!(notes.iter().any(|n| n.contains("Unable to verify")));
    assert!(notes.iter().any(|n| n.contains("RPC error")));
}

#[test]
fn test_safety_score_duration_bonus() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test 100% locked with 0 days - should have no duration bonus
    let locked_0_days = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 0,
        expiry_timestamp: 0,
        percentage_locked: 100,
    };
    assert_eq!(verifier.calculate_safety_score(&locked_0_days), 80);

    // Test 100% locked with 182 days (half year) - should have ~10 point bonus
    let locked_half_year = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 182 * 24 * 60 * 60,
        expiry_timestamp: 0,
        percentage_locked: 100,
    };
    let score = verifier.calculate_safety_score(&locked_half_year);
    assert!(score >= 89 && score <= 91, "Expected ~90, got {}", score);

    // Test 100% locked with 2 years - should have max 20 point bonus
    let locked_2_years = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 730 * 24 * 60 * 60,
        expiry_timestamp: 0,
        percentage_locked: 100,
    };
    assert_eq!(verifier.calculate_safety_score(&locked_2_years), 100);
}

#[test]
fn test_risk_level_boundary_conditions() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test exactly 95% locked - should be Minimal
    let locked_95 = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 365 * 24 * 60 * 60,
        expiry_timestamp: 0,
        percentage_locked: 95,
    };
    assert_eq!(
        verifier.calculate_risk_level(&locked_95),
        RiskLevel::Minimal
    );

    // Test exactly 94% locked - should be Low (not Minimal)
    let locked_94 = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 365 * 24 * 60 * 60,
        expiry_timestamp: 0,
        percentage_locked: 94,
    };
    assert_eq!(verifier.calculate_risk_level(&locked_94), RiskLevel::Low);

    // Test exactly 80% locked - should be Low
    let locked_80 = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 365 * 24 * 60 * 60,
        expiry_timestamp: 0,
        percentage_locked: 80,
    };
    assert_eq!(verifier.calculate_risk_level(&locked_80), RiskLevel::Low);

    // Test exactly 79% locked - should be Medium
    let locked_79 = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 365 * 24 * 60 * 60,
        expiry_timestamp: 0,
        percentage_locked: 79,
    };
    assert_eq!(verifier.calculate_risk_level(&locked_79), RiskLevel::Medium);

    // Test exactly 50% locked - should be Medium
    let locked_50 = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 365 * 24 * 60 * 60,
        expiry_timestamp: 0,
        percentage_locked: 50,
    };
    assert_eq!(verifier.calculate_risk_level(&locked_50), RiskLevel::Medium);

    // Test exactly 49% locked - should be High
    let locked_49 = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 365 * 24 * 60 * 60,
        expiry_timestamp: 0,
        percentage_locked: 49,
    };
    assert_eq!(verifier.calculate_risk_level(&locked_49), RiskLevel::High);
}

#[test]
fn test_secured_address_detection() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test burn addresses (using literal addresses for test clarity)
    assert!(verifier.is_address_secured("11111111111111111111111111111111")); // System Program
    assert!(verifier.is_address_secured("1nc1nerator11111111111111111111111111111111")); // Incinerator

    // Test lock programs
    assert!(verifier.is_address_secured("LocktDzaV1W2Bm9DeZeiyz4J9zs4fRqNiYqQyracRXw")); // Streamflow

    // Test DEX programs
    assert!(verifier.is_address_secured("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8")); // Raydium
    assert!(verifier.is_address_secured("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc")); // Orca

    // Test farm programs
    assert!(verifier.is_address_secured("EhhTKczWMGQt46ynNeRX1WfeagwwJd7ufHvCDjRxjo5Q")); // Raydium Farm

    // Test unsecured address (random user wallet)
    assert!(!verifier.is_address_secured("SomeRandomUserWalletAddress1234567890ABC"));
}

#[test]
fn test_edge_case_expired_lock() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test lock that has technically expired (0 seconds remaining)
    let expired_lock = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 0,                                               // Expired
        expiry_timestamp: chrono::Utc::now().timestamp() as u64 - 1000, // Past
        percentage_locked: 100,
    };

    // Even expired, if 100% locked it should not be Critical
    // (Contract enforcement would prevent withdrawal regardless)
    let risk = verifier.calculate_risk_level(&expired_lock);
    assert!(risk != RiskLevel::Minimal); // Should not be minimal with 0 duration

    let score = verifier.calculate_safety_score(&expired_lock);
    assert!(score > 0); // Should have some score based on percentage
}

#[test]
fn test_multiple_lock_sources() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test partial: both locked and burned
    let multi_lock = LockStatus::Partial {
        locked_percentage: 60,
        burned_percentage: 40,
        lock_info: Some(Box::new(LockStatus::Locked {
            contract: "test".to_string(),
            duration_secs: 365 * 24 * 60 * 60,
            expiry_timestamp: 0,
            percentage_locked: 60,
        })),
    };

    assert_eq!(
        verifier.calculate_risk_level(&multi_lock),
        RiskLevel::Minimal
    ); // 100% total
    assert_eq!(verifier.calculate_safety_score(&multi_lock), 100); // 60 + 40 = 100
    assert!(!verifier.should_auto_reject(&multi_lock, &RiskLevel::Minimal));
}

#[test]
fn test_platform_specific_verification() {
    let config = LpLockConfig::default();

    // Verify the default configuration is correct
    assert_eq!(config.timeout_secs, 5);
    assert_eq!(config.min_lock_percentage, 80);
    assert_eq!(config.min_lock_duration_days, 180);

    // Create verifier with the config
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let _verifier = LpLockVerifier::new(config, rpc);

    // Just verify the verifier is created properly for different platforms
    // Actual platform verification requires real RPC calls which we can't mock here
}

#[test]
fn test_conservative_defaults() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Unknown status should have zero safety score (conservative)
    let unknown = LockStatus::Unknown {
        reason: "RPC error".to_string(),
    };
    assert_eq!(verifier.calculate_safety_score(&unknown), 0);
    assert_eq!(verifier.calculate_risk_level(&unknown), RiskLevel::Critical);
    assert!(verifier.should_auto_reject(&unknown, &RiskLevel::Critical));
}

#[test]
fn test_notes_quality() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test that notes contain useful information
    let partial = LockStatus::Partial {
        locked_percentage: 50,
        burned_percentage: 30,
        lock_info: None,
    };
    let risk = verifier.calculate_risk_level(&partial);
    let notes = verifier.generate_notes(&partial, &risk);

    // Should mention both locked and burned
    assert!(notes.iter().any(|n| n.contains("50%")));
    assert!(notes.iter().any(|n| n.contains("30%")));
    assert!(notes.iter().any(|n| n.contains("Partial security")));
}

#[test]
fn test_config_customization() {
    let custom_config = LpLockConfig {
        timeout_secs: 10,
        min_lock_percentage: 90,
        min_lock_duration_days: 365,
        auto_reject_threshold: 70,
        ..Default::default()
    };
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(custom_config.clone(), rpc);

    // Verify custom config is used
    let locked_85 = LockStatus::Locked {
        contract: "test".to_string(),
        duration_secs: 365 * 24 * 60 * 60,
        expiry_timestamp: 0,
        percentage_locked: 85,
    };

    // With 90% min threshold, 85% locked should be Medium or worse (not Low/Minimal)
    let risk = verifier.calculate_risk_level(&locked_85);
    assert!(
        risk >= RiskLevel::Medium,
        "Expected Medium or worse risk, got {:?}",
        risk
    );
}

#[test]
fn test_integer_math_precision() {
    let config = LpLockConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let verifier = LpLockVerifier::new(config, rpc);

    // Test that percentages are calculated correctly
    // 50% locked + 49% burned = 99% secured
    let partial = LockStatus::Partial {
        locked_percentage: 50,
        burned_percentage: 49,
        lock_info: None,
    };

    let score = verifier.calculate_safety_score(&partial);
    assert_eq!(score, 99); // Should be exact

    let risk = verifier.calculate_risk_level(&partial);
    assert_eq!(risk, RiskLevel::Minimal); // 99% >= 95%
}
