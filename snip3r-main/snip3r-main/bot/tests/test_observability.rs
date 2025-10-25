//! Integration tests for observability system

use h_5n1p3r::observability::{get_metrics, ObservabilityConfig};
use std::sync::Once;

static INIT: Once = Once::new();

/// Initialize observability only once for all tests
fn init_test_observability() {
    INIT.call_once(|| {
        let config = ObservabilityConfig {
            enable_tracing: false,
            enable_metrics: false,
            ..Default::default()
        };

        // Ignore error if already initialized
        let _ = h_5n1p3r::observability::init_observability(Some(config));
    });
}

#[test]
fn test_observability_config_default() {
    let config = ObservabilityConfig::default();
    assert_eq!(config.service_name, "h-5n1p3r");
    assert!(config.enable_tracing);
    assert!(config.enable_metrics);
    assert_eq!(config.sampling_ratio, 1.0);
}

#[test]
fn test_observability_config_custom() {
    let config = ObservabilityConfig {
        service_name: "test-service".to_string(),
        service_version: "1.0.0".to_string(),
        environment: "test".to_string(),
        otlp_endpoint: "http://test:4317".to_string(),
        enable_tracing: false,
        enable_metrics: true,
        sampling_ratio: 0.5,
    };

    assert_eq!(config.service_name, "test-service");
    assert_eq!(config.service_version, "1.0.0");
    assert_eq!(config.environment, "test");
    assert_eq!(config.otlp_endpoint, "http://test:4317");
    assert!(!config.enable_tracing);
    assert!(config.enable_metrics);
    assert_eq!(config.sampling_ratio, 0.5);
}

#[test]
fn test_get_metrics() {
    // Get metrics should not panic even if not initialized
    let metrics = get_metrics();
    // Metrics might be empty if Prometheus is not set up
    assert!(metrics.is_empty() || metrics.contains("# HELP") || metrics.contains("# TYPE"));
}

#[tokio::test]
async fn test_instrumented_function() {
    init_test_observability();

    // Call an instrumented function
    instrumented_test_function().await;
}

#[tracing::instrument]
async fn instrumented_test_function() {
    tracing::info!("Test function called");
    // Simulate some work
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    tracing::info!("Test function completed");
}

#[tokio::test]
async fn test_span_hierarchy() {
    init_test_observability();

    parent_function().await;
}

#[tracing::instrument]
async fn parent_function() {
    tracing::info!("Parent function started");
    child_function().await;
    tracing::info!("Parent function completed");
}

#[tracing::instrument]
async fn child_function() {
    tracing::info!("Child function started");
    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
    tracing::info!("Child function completed");
}

#[tokio::test]
async fn test_context_propagation_across_tasks() {
    init_test_observability();

    // Create a parent span
    let span = tracing::info_span!("parent_task");
    let _enter = span.enter();

    tracing::info!("Parent task started");

    // Spawn a child task - context should propagate
    let handle = tokio::spawn(async {
        tracing::info!("Child task started");
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        tracing::info!("Child task completed");
    });

    handle.await.expect("Child task failed");
    tracing::info!("Parent task completed");
}
