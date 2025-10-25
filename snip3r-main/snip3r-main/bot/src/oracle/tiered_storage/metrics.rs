//! Metrics and Monitoring for Tiered Storage
//!
//! This module provides per-tier metrics collection and monitoring capabilities.

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::traits::TierMetricsSnapshot;

/// Monitor for tracking and alerting on tier metrics
pub struct TierMonitor {
    hot_thresholds: TierThresholds,
    warm_thresholds: TierThresholds,
    cold_thresholds: TierThresholds,
    alerts: Arc<RwLock<Vec<TierAlert>>>,
}

/// Thresholds for tier performance monitoring
#[derive(Debug, Clone)]
pub struct TierThresholds {
    /// Maximum acceptable P95 access time in microseconds
    pub max_p95_us: u64,

    /// Maximum acceptable P99 access time in microseconds
    pub max_p99_us: u64,

    /// Minimum acceptable hit rate (0.0 to 1.0)
    pub min_hit_rate: f64,

    /// Maximum capacity utilization before warning (0.0 to 1.0)
    pub max_capacity_util: f64,
}

/// Alert generated when thresholds are exceeded
#[derive(Debug, Clone)]
pub struct TierAlert {
    pub tier_name: String,
    pub severity: AlertSeverity,
    pub message: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

/// Aggregated metrics across all tiers
#[derive(Debug, Clone)]
pub struct TierMetrics {
    pub hot: Option<TierMetricsSnapshot>,
    pub warm: Option<TierMetricsSnapshot>,
    pub cold: Option<TierMetricsSnapshot>,
}

impl TierMonitor {
    /// Create a new tier monitor with default thresholds
    pub fn new() -> Self {
        Self {
            hot_thresholds: TierThresholds {
                max_p95_us: 1000,        // 1ms
                max_p99_us: 2000,        // 2ms
                min_hit_rate: 0.80,      // 80%
                max_capacity_util: 0.90, // 90%
            },
            warm_thresholds: TierThresholds {
                max_p95_us: 10_000,      // 10ms
                max_p99_us: 20_000,      // 20ms
                min_hit_rate: 0.90,      // 90%
                max_capacity_util: 0.85, // 85%
            },
            cold_thresholds: TierThresholds {
                max_p95_us: 100_000,     // 100ms
                max_p99_us: 200_000,     // 200ms
                min_hit_rate: 0.70,      // 70%
                max_capacity_util: 0.95, // 95%
            },
            alerts: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Check metrics against thresholds and generate alerts
    pub async fn check_metrics(&self, metrics: &TierMetrics) {
        if let Some(ref hot) = metrics.hot {
            self.check_tier_metrics("Hot (L1)", hot, &self.hot_thresholds)
                .await;
        }

        if let Some(ref warm) = metrics.warm {
            self.check_tier_metrics("Warm (L2)", warm, &self.warm_thresholds)
                .await;
        }

        if let Some(ref cold) = metrics.cold {
            self.check_tier_metrics("Cold (L3)", cold, &self.cold_thresholds)
                .await;
        }
    }

    /// Check individual tier metrics
    async fn check_tier_metrics(
        &self,
        tier_name: &str,
        metrics: &TierMetricsSnapshot,
        thresholds: &TierThresholds,
    ) {
        let mut alerts = self.alerts.write().await;
        let now = chrono::Utc::now().timestamp() as u64;

        // Check P95 access time
        if metrics.p95_access_time_us > thresholds.max_p95_us {
            let alert = TierAlert {
                tier_name: tier_name.to_string(),
                severity: AlertSeverity::Warning,
                message: format!(
                    "P95 access time {}µs exceeds threshold {}µs",
                    metrics.p95_access_time_us, thresholds.max_p95_us
                ),
                timestamp: now,
            };
            warn!("{}: {}", alert.tier_name, alert.message);
            alerts.push(alert);
        }

        // Check P99 access time
        if metrics.p99_access_time_us > thresholds.max_p99_us {
            let alert = TierAlert {
                tier_name: tier_name.to_string(),
                severity: AlertSeverity::Critical,
                message: format!(
                    "P99 access time {}µs exceeds threshold {}µs",
                    metrics.p99_access_time_us, thresholds.max_p99_us
                ),
                timestamp: now,
            };
            warn!("{}: {}", alert.tier_name, alert.message);
            alerts.push(alert);
        }

        // Check hit rate
        let hit_rate = metrics.hit_rate() / 100.0;
        if hit_rate < thresholds.min_hit_rate {
            let alert = TierAlert {
                tier_name: tier_name.to_string(),
                severity: AlertSeverity::Warning,
                message: format!(
                    "Hit rate {:.2}% is below threshold {:.2}%",
                    hit_rate * 100.0,
                    thresholds.min_hit_rate * 100.0
                ),
                timestamp: now,
            };
            warn!("{}: {}", alert.tier_name, alert.message);
            alerts.push(alert);
        }

        // Keep only recent alerts (last 100)
        if alerts.len() > 100 {
            let excess = alerts.len() - 100;
            alerts.drain(0..excess);
        }
    }

    /// Get recent alerts
    pub async fn get_alerts(&self) -> Vec<TierAlert> {
        self.alerts.read().await.clone()
    }

    /// Clear all alerts
    pub async fn clear_alerts(&self) {
        self.alerts.write().await.clear();
    }

    /// Log a summary of metrics
    pub fn log_metrics_summary(&self, metrics: &TierMetrics) {
        info!("=== Tiered Storage Metrics Summary ===");

        if let Some(ref hot) = metrics.hot {
            info!(
                "Hot (L1): {} records, {:.2}% hit rate, P95: {}µs, P99: {}µs",
                hot.record_count,
                hot.hit_rate(),
                hot.p95_access_time_us,
                hot.p99_access_time_us
            );
        }

        if let Some(ref warm) = metrics.warm {
            info!(
                "Warm (L2): {} records, {:.2}% hit rate, P95: {}µs, P99: {}µs",
                warm.record_count,
                warm.hit_rate(),
                warm.p95_access_time_us,
                warm.p99_access_time_us
            );
        }

        if let Some(ref cold) = metrics.cold {
            info!(
                "Cold (L3): {} records, {:.2}% hit rate, P95: {}µs, P99: {}µs",
                cold.record_count,
                cold.hit_rate(),
                cold.p95_access_time_us,
                cold.p99_access_time_us
            );
        }

        info!("=====================================");
    }
}

impl Default for TierMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_monitor_alerts_on_threshold_violation() {
        let monitor = TierMonitor::new();

        let bad_metrics = TierMetricsSnapshot {
            record_count: 100,
            access_count: 100,
            hit_count: 50, // 50% hit rate, below 80% threshold
            miss_count: 50,
            avg_access_time_us: 2000,
            p95_access_time_us: 3000, // Exceeds 1ms threshold
            p99_access_time_us: 5000, // Exceeds 2ms threshold
            bytes_stored: 10000,
            eviction_count: 10,
        };

        let metrics = TierMetrics {
            hot: Some(bad_metrics),
            warm: None,
            cold: None,
        };

        monitor.check_metrics(&metrics).await;

        let alerts = monitor.get_alerts().await;
        assert!(!alerts.is_empty(), "Should have generated alerts");

        // Should have alerts for P95, P99, and hit rate
        assert!(alerts.len() >= 3, "Should have at least 3 alerts");
    }

    #[tokio::test]
    async fn test_monitor_no_alerts_within_thresholds() {
        let monitor = TierMonitor::new();

        let good_metrics = TierMetricsSnapshot {
            record_count: 100,
            access_count: 100,
            hit_count: 90, // 90% hit rate
            miss_count: 10,
            avg_access_time_us: 500,
            p95_access_time_us: 800, // Under 1ms
            p99_access_time_us: 950, // Under 2ms
            bytes_stored: 10000,
            eviction_count: 5,
        };

        let metrics = TierMetrics {
            hot: Some(good_metrics),
            warm: None,
            cold: None,
        };

        monitor.check_metrics(&metrics).await;

        let alerts = monitor.get_alerts().await;
        assert_eq!(
            alerts.len(),
            0,
            "Should not generate alerts for good metrics"
        );
    }
}
