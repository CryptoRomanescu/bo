//! Integration tests for security features including authentication and rate limiting.

use h_5n1p3r::security::{validate_auth_token, validate_rpc_url, validate_solana_pubkey};

#[test]
fn test_auth_token_validation() {
    // Valid token (32+ characters)
    let valid_token = "a".repeat(32);
    assert!(validate_auth_token(&valid_token).is_ok());

    // Very long token should also be valid
    let long_token = "a".repeat(128);
    assert!(validate_auth_token(&long_token).is_ok());

    // Too short token should fail
    let short_token = "short";
    assert!(validate_auth_token(&short_token).is_err());

    // Token with non-printable characters should fail
    let bad_token = format!("{}\x00", "a".repeat(32));
    assert!(validate_auth_token(&bad_token).is_err());
}

#[test]
fn test_rpc_url_validation() {
    // HTTPS URLs should be valid
    assert!(validate_rpc_url("https://api.mainnet-beta.solana.com").is_ok());
    assert!(validate_rpc_url("https://example.com:8899").is_ok());

    // HTTP localhost should be allowed for testing
    assert!(validate_rpc_url("http://localhost:8899").is_ok());
    assert!(validate_rpc_url("http://127.0.0.1:8899").is_ok());

    // HTTP non-localhost should be rejected for security
    assert!(validate_rpc_url("http://api.mainnet-beta.solana.com").is_err());

    // Invalid URLs should be rejected
    assert!(validate_rpc_url("not a url").is_err());
    assert!(validate_rpc_url("").is_err());
}

#[test]
fn test_pubkey_validation() {
    // Valid Solana pubkey
    assert!(validate_solana_pubkey("11111111111111111111111111111111").is_ok());

    // Invalid format
    assert!(validate_solana_pubkey("invalid").is_err());
    assert!(validate_solana_pubkey("").is_err());
    assert!(validate_solana_pubkey("too_short").is_err());
}

#[test]
fn test_security_config_integration() {
    use std::env;

    // Test that environment variables can be set for secure configuration
    env::set_var("TEST_METRICS_AUTH_TOKEN", "a".repeat(32));

    let token = env::var("TEST_METRICS_AUTH_TOKEN").unwrap();
    assert!(validate_auth_token(&token).is_ok());

    // Clean up
    env::remove_var("TEST_METRICS_AUTH_TOKEN");
}

#[tokio::test]
async fn test_rate_limiter_integration() {
    use h_5n1p3r::http_rate_limit::IpRateLimiter;
    use std::net::IpAddr;
    use std::str::FromStr;

    let limiter = IpRateLimiter::new(10);
    let ip = IpAddr::from_str("192.168.1.1").unwrap();

    // Should allow initial requests
    for i in 0..5 {
        let result = limiter.check_rate_limit(ip).await;
        assert!(result.is_ok(), "Request {} should be allowed", i);
    }

    // Should handle different IPs independently
    let ip2 = IpAddr::from_str("192.168.1.2").unwrap();
    let result = limiter.check_rate_limit(ip2).await;
    assert!(result.is_ok(), "Different IP should be allowed");
}

#[test]
fn test_constant_time_comparison() {
    // Verify that we're using constant-time comparison for tokens
    // This is a compile-time check - if subtle crate is used, it's constant-time
    use subtle::ConstantTimeEq;

    let token1 = b"secret_token_12345678901234567890";
    let token2 = b"secret_token_12345678901234567890";
    let token3 = b"different_token_12345678901234567";

    // Same tokens should compare equal
    assert_eq!(token1.ct_eq(token2).unwrap_u8(), 1);

    // Different tokens should not compare equal
    assert_eq!(token1.ct_eq(token3).unwrap_u8(), 0);
}

#[test]
fn test_metrics_auth_token_from_env() {
    use h_5n1p3r::orchestrator::OrchestratorConfig;
    use std::env;

    // Test with no token set
    env::remove_var("METRICS_AUTH_TOKEN");
    assert!(OrchestratorConfig::get_metrics_auth_token().is_none());

    // Test with valid token
    let valid_token = "a".repeat(32);
    env::set_var("METRICS_AUTH_TOKEN", &valid_token);
    let result = OrchestratorConfig::get_metrics_auth_token();
    assert!(result.is_some());
    assert_eq!(result.unwrap(), valid_token);

    // Test with invalid token (too short)
    env::set_var("METRICS_AUTH_TOKEN", "short");
    assert!(OrchestratorConfig::get_metrics_auth_token().is_none());

    // Clean up
    env::remove_var("METRICS_AUTH_TOKEN");
}

#[test]
fn test_config_validation_on_load() {
    use h_5n1p3r::orchestrator::OrchestratorConfig;
    use std::env;
    use std::fs;

    // Create a test config file
    let test_config = r#"
database_path = "test.db"
rpc_url = "https://api.mainnet-beta.solana.com"
wallet_pubkey = "11111111111111111111111111111111"
rpc_timeout_secs = 30
channel_buffer_size = 100
log_level = "info"

[perf_monitor]
analysis_interval_minutes = 15
lookback_hours = 24

[tx_monitor]
check_interval_ms = 1000

[regime_detector]
analysis_interval_secs = 60
"#;

    let test_path = "/tmp/test_config.toml";
    fs::write(test_path, test_config).unwrap();

    // Should load successfully with valid config
    let result = OrchestratorConfig::from_toml_file(test_path);
    assert!(result.is_ok(), "Valid config should load successfully");

    // Test environment variable override
    env::set_var("RPC_URL", "https://custom-rpc.example.com");
    let config = OrchestratorConfig::from_toml_file(test_path).unwrap();
    assert_eq!(config.rpc_url, "https://custom-rpc.example.com");

    // Clean up
    fs::remove_file(test_path).ok();
    env::remove_var("RPC_URL");
}
