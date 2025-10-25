//! Demo of advanced circuit breaker features with adaptive thresholds and health scores
//!
//! This example demonstrates:
//! 1. Adaptive thresholds with EWMA
//! 2. Health score calculation from latency, errors, and jitter
//! 3. Prometheus metrics export
//! 4. Structured logging

use h_5n1p3r::oracle::circuit_breaker::{CircuitBreaker, EndpointState};
use rand::Rng;
use std::time::Instant;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for structured logging
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(false)
        .with_level(true)
        .json()
        .init();

    println!("=== Circuit Breaker Advanced Features Demo ===\n");

    // Create circuit breaker with custom EWMA and health score configuration
    let cb = CircuitBreaker::with_full_config(
        5,    // failure_threshold
        10,   // cooldown_seconds (shorter for demo)
        50,   // sample_size
        3,    // canary_count
        true, // adaptive_thresholds enabled
        0.3,  // ewma_alpha (30% weight on recent values)
        2.0,  // adaptive_sensitivity (2 standard deviations)
        0.5,  // min_health_score (require at least 50% health)
    );

    let config = cb.get_config();
    println!("Configuration:");
    println!("  Failure threshold: {}", config.failure_threshold);
    println!("  EWMA alpha: {}", config.ewma_alpha);
    println!("  Adaptive sensitivity: {}", config.adaptive_sensitivity);
    println!("  Min health score: {}\n", config.min_health_score);

    // Simulate different endpoint scenarios
    println!("Scenario 1: Healthy endpoint with low latency");
    simulate_healthy_endpoint(&cb).await;
    print_endpoint_stats(&cb, "healthy-endpoint").await;

    println!("\nScenario 2: Endpoint with occasional failures (spike)");
    simulate_spike(&cb).await;
    print_endpoint_stats(&cb, "spike-endpoint").await;

    println!("\nScenario 3: Chronically degraded endpoint");
    simulate_chronic_degradation(&cb).await;
    print_endpoint_stats(&cb, "degraded-endpoint").await;

    println!("\nScenario 4: High latency endpoint");
    simulate_high_latency(&cb).await;
    print_endpoint_stats(&cb, "slow-endpoint").await;

    // Export Prometheus metrics
    println!("\n=== Prometheus Metrics ===");
    let registry = cb.get_metrics_registry().await;
    let metrics = registry.gather();

    println!("Total metrics families: {}", metrics.len());
    for metric_family in metrics.iter() {
        println!(
            "  - {}: {}",
            metric_family.get_name(),
            metric_family.get_help()
        );
    }

    println!("\n=== Demo Complete ===");
    Ok(())
}

async fn simulate_healthy_endpoint(cb: &CircuitBreaker) {
    let endpoint = "healthy-endpoint";
    let mut rng = rand::thread_rng();

    for _ in 0..20 {
        let latency = rng.gen_range(30.0..80.0); // Low latency
        cb.record_success_with_latency(endpoint, Some(latency))
            .await;
        sleep(Duration::from_millis(10)).await;
    }
}

async fn simulate_spike(cb: &CircuitBreaker) {
    let endpoint = "spike-endpoint";
    let mut rng = rand::thread_rng();

    // Normal operation
    for _ in 0..15 {
        let latency = rng.gen_range(50.0..100.0);
        cb.record_success_with_latency(endpoint, Some(latency))
            .await;
        sleep(Duration::from_millis(10)).await;
    }

    // Brief spike of failures
    for _ in 0..5 {
        let latency = rng.gen_range(200.0..500.0);
        cb.record_failure_with_latency(endpoint, Some(latency))
            .await;
        sleep(Duration::from_millis(10)).await;
    }

    // Recovery
    for _ in 0..15 {
        let latency = rng.gen_range(50.0..100.0);
        cb.record_success_with_latency(endpoint, Some(latency))
            .await;
        sleep(Duration::from_millis(10)).await;
    }
}

async fn simulate_chronic_degradation(cb: &CircuitBreaker) {
    let endpoint = "degraded-endpoint";
    let mut rng = rand::thread_rng();

    // Consistent 60% error rate
    for i in 0..30 {
        let latency = rng.gen_range(100.0..300.0);
        if i % 5 < 2 {
            cb.record_success_with_latency(endpoint, Some(latency))
                .await;
        } else {
            cb.record_failure_with_latency(endpoint, Some(latency))
                .await;
        }
        sleep(Duration::from_millis(10)).await;
    }
}

async fn simulate_high_latency(cb: &CircuitBreaker) {
    let endpoint = "slow-endpoint";
    let mut rng = rand::thread_rng();

    // High latency with high jitter
    for _ in 0..20 {
        let latency = rng.gen_range(1500.0..3500.0); // Very high latency
        if rng.gen_bool(0.8) {
            // 80% success rate
            cb.record_success_with_latency(endpoint, Some(latency))
                .await;
        } else {
            cb.record_failure_with_latency(endpoint, Some(latency))
                .await;
        }
        sleep(Duration::from_millis(10)).await;
    }
}

async fn print_endpoint_stats(cb: &CircuitBreaker, endpoint: &str) {
    let stats = cb.get_health_stats().await;
    if let Some(stat) = stats.get(endpoint) {
        println!("  Endpoint: {}", endpoint);
        println!("    State: {:?}", stat.state);
        println!("    Success rate: {:.2}%", stat.success_rate * 100.0);
        println!("    Health score: {:.3}", stat.health_score);
        println!("    Avg latency: {:.2}ms", stat.avg_latency_ms);
        println!("    Latency jitter: {:.2}ms", stat.latency_jitter_ms);
        println!("    EWMA error rate: {:.3}", stat.ewma_error_rate);
        println!("    Adaptive threshold: {}", stat.adaptive_threshold);
        println!("    Consecutive failures: {}", stat.consecutive_failures);

        // Interpret health score
        let health_interpretation = if stat.health_score >= 0.8 {
            "Excellent"
        } else if stat.health_score >= 0.6 {
            "Good"
        } else if stat.health_score >= 0.4 {
            "Fair"
        } else if stat.health_score >= 0.2 {
            "Poor"
        } else {
            "Critical"
        };
        println!("    Health interpretation: {}", health_interpretation);
    }
}
