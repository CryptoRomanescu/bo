//! Basic tests for the actor system foundation
//!
//! These tests verify that the actor system components can be created
//! and basic message passing works correctly.

use actix::prelude::*;
use h_5n1p3r::actors::*;
use h_5n1p3r::oracle::quantum_oracle::SimpleOracleConfig;
use h_5n1p3r::types::Pubkey;
use tokio::sync::mpsc;

#[actix::test]
async fn test_oracle_actor_creation() {
    // Create a channel for scored candidates
    let (scored_sender, _scored_receiver) = mpsc::channel(10);

    // Create oracle configuration
    let config = SimpleOracleConfig::default();

    // Create OracleActor
    let oracle_actor = OracleActor::new(config, scored_sender);

    assert!(oracle_actor.is_ok(), "OracleActor creation should succeed");
}

#[actix::test]
async fn test_storage_actor_creation() {
    // Create StorageActor
    let storage_actor = StorageActor::new().await;

    assert!(
        storage_actor.is_ok(),
        "StorageActor creation should succeed"
    );
}

#[actix::test]
async fn test_supervisor_actor_creation() {
    // Create configuration
    let oracle_config = SimpleOracleConfig::default();
    let rpc_url = "https://api.mainnet-beta.solana.com".to_string();
    let wallet_pubkey = Pubkey::try_from("11111111111111111111111111111112").unwrap();
    let check_interval_ms = 1000;
    let (scored_sender, _scored_receiver) = mpsc::channel(10);

    // Create SupervisorActor
    let supervisor = SupervisorActor::new(
        SupervisionStrategy::RestartImmediately,
        oracle_config,
        rpc_url,
        wallet_pubkey,
        check_interval_ms,
        scored_sender,
    );

    // Start the supervisor
    let _addr = supervisor.start();

    // Test passes if we reach here without panicking
}

#[actix::test]
async fn test_get_system_health() {
    // Create configuration
    let oracle_config = SimpleOracleConfig::default();
    let rpc_url = "https://api.mainnet-beta.solana.com".to_string();
    let wallet_pubkey = Pubkey::try_from("11111111111111111111111111111112").unwrap();
    let check_interval_ms = 1000;
    let (scored_sender, _scored_receiver) = mpsc::channel(10);

    // Create and start SupervisorActor
    let supervisor = SupervisorActor::new(
        SupervisionStrategy::RestartImmediately,
        oracle_config,
        rpc_url,
        wallet_pubkey,
        check_interval_ms,
        scored_sender,
    );

    let addr = supervisor.start();

    // Query system health
    let health = addr.send(GetSystemHealth).await;

    assert!(health.is_ok(), "GetSystemHealth message should succeed");

    let health = health.unwrap();
    assert_eq!(
        health.uptime_secs >= 0,
        true,
        "Uptime should be non-negative"
    );
}

#[actix::test]
async fn test_oracle_metrics() {
    // Create a channel for scored candidates
    let (scored_sender, _scored_receiver) = mpsc::channel(10);

    // Create oracle configuration
    let config = SimpleOracleConfig::default();

    // Create and start OracleActor
    let oracle_actor = OracleActor::new(config, scored_sender).unwrap();
    let addr = oracle_actor.start();

    // Get metrics
    let metrics = addr.send(GetOracleMetrics).await;

    assert!(metrics.is_ok(), "GetOracleMetrics message should succeed");

    let metrics = metrics.unwrap();
    assert_eq!(
        metrics.total_scored, 0,
        "Initially should have 0 scored candidates"
    );
}

#[actix::test]
async fn test_storage_stats() {
    // Create and start StorageActor
    let storage_actor = StorageActor::new().await.unwrap();
    let addr = storage_actor.start();

    // Get storage stats
    let stats = addr.send(GetStorageStats).await;

    assert!(stats.is_ok(), "GetStorageStats message should succeed");

    let stats = stats.unwrap();
    assert!(
        stats.total_decisions >= 0,
        "Total decisions should be non-negative"
    );
}
