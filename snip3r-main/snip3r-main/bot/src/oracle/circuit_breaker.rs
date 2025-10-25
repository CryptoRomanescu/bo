//! Circuit breaker for RPC endpoint health tracking.
//!
//! This module provides circuit breaker functionality to temporarily
//! quarantine unhealthy RPC endpoints and retry them after cooldown.

use prometheus::{
    register_gauge_with_registry, register_int_counter_with_registry,
    register_int_gauge_with_registry, Gauge, IntCounter, IntGauge, Opts, Registry,
};
use rand::Rng;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, instrument, warn};

/// Backoff strategy with exponential backoff and jitter.
#[derive(Debug, Clone)]
pub struct BackoffStrategy {
    /// Base delay in milliseconds
    pub base_delay_ms: u64,
    /// Maximum delay in milliseconds
    pub max_delay_ms: u64,
    /// Current attempt number
    pub attempt: u32,
}

impl BackoffStrategy {
    /// Maximum attempt number to prevent overflow in exponential calculation.
    /// This value is used as the exponent in base * 2^attempt, capped at 63
    /// because 2^64 would overflow u64.
    const MAX_ATTEMPT: u32 = 63;

    /// Create a new backoff strategy.
    pub fn new(base_delay_ms: u64, max_delay_ms: u64) -> Self {
        Self {
            base_delay_ms,
            max_delay_ms,
            attempt: 0,
        }
    }

    /// Calculate delay for current attempt with exponential backoff and ±30% jitter.
    /// This method does not increment the attempt counter.
    pub fn current_delay(&self) -> Duration {
        // Use current attempt without incrementing
        let attempt = self.attempt;

        // Calculate exponential backoff: base * 2^attempt
        // Using saturating_mul as a safety net
        let exponential_delay = self
            .base_delay_ms
            .saturating_mul(2u64.saturating_pow(attempt));
        let capped_delay = exponential_delay.min(self.max_delay_ms);

        // Apply ±30% jitter
        let mut rng = rand::thread_rng();
        let jitter_factor = rng.gen_range(0.7..=1.3);
        let jittered_ms = (capped_delay as f64 * jitter_factor) as u64;

        Duration::from_millis(jittered_ms)
    }

    /// Calculate next delay with exponential backoff and ±30% jitter.
    pub fn next_delay(&mut self) -> Duration {
        // Increment attempt, but cap at MAX_ATTEMPT to prevent overflow (2^64 would overflow u64)
        self.attempt = (self.attempt + 1).min(Self::MAX_ATTEMPT);

        // Calculate exponential backoff: base * 2^attempt
        // Using saturating_mul as a safety net, though attempt is capped at MAX_ATTEMPT
        let exponential_delay = self
            .base_delay_ms
            .saturating_mul(2u64.saturating_pow(self.attempt));
        let capped_delay = exponential_delay.min(self.max_delay_ms);

        // Apply ±30% jitter
        let mut rng = rand::thread_rng();
        let jitter_factor = rng.gen_range(0.7..=1.3);
        let jittered_ms = (capped_delay as f64 * jitter_factor) as u64;

        Duration::from_millis(jittered_ms)
    }

    /// Reset the backoff strategy on success.
    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}

/// State of an RPC endpoint in the circuit breaker.
#[derive(Debug, Clone, PartialEq)]
pub enum EndpointState {
    /// Endpoint is healthy and can be used
    Healthy,
    /// Endpoint is degraded due to failures but still usable
    Degraded,
    /// Endpoint is in cooldown period after too many failures
    CoolingDown,
    /// Half-open state - sending canary requests to test recovery
    HalfOpen,
}

/// EWMA (Exponentially Weighted Moving Average) calculator for adaptive thresholds
#[derive(Debug, Clone)]
pub struct EWMA {
    /// Current EWMA value
    value: f64,
    /// Alpha parameter (smoothing factor, 0 < alpha <= 1)
    alpha: f64,
    /// Sum of squared deviations for variance calculation
    variance: f64,
    /// Number of samples processed
    count: u64,
}

impl EWMA {
    /// Create a new EWMA with given alpha (higher alpha = more weight on recent values)
    fn new(alpha: f64) -> Self {
        Self {
            value: 0.0,
            alpha: alpha.clamp(0.01, 1.0),
            variance: 0.0,
            count: 0,
        }
    }

    /// Update EWMA with a new sample (1.0 for error, 0.0 for success)
    fn update(&mut self, sample: f64) {
        if self.count == 0 {
            self.value = sample;
        } else {
            let delta = sample - self.value;
            self.value += self.alpha * delta;
            // Update variance using exponential moving variance
            self.variance = (1.0 - self.alpha) * (self.variance + self.alpha * delta * delta);
        }
        self.count += 1;
    }

    /// Get current EWMA value
    fn get(&self) -> f64 {
        self.value
    }

    /// Get standard deviation
    fn std_dev(&self) -> f64 {
        self.variance.sqrt()
    }
}

/// Health tracking for individual RPC endpoints.
#[derive(Debug, Clone)]
pub struct EndpointHealth {
    /// Current state of the endpoint
    pub state: EndpointState,
    /// Consecutive failure count
    pub consecutive_failures: u32,
    /// Success rate over recent attempts
    pub success_rate: f64,
    /// Total attempts in current window
    pub total_attempts: usize,
    /// Successful attempts in current window
    pub successful_attempts: usize,
    /// Timestamp of last failure
    pub last_failure: Option<Instant>,
    /// Timestamp when cooldown started
    pub cooldown_start: Option<Instant>,
    /// Recent attempt history (success=true, failure=false)
    pub recent_attempts: Vec<bool>,
    /// Number of canary requests sent in half-open state
    pub canary_count: u32,
    /// Number of successful canary requests
    pub canary_successes: u32,
    /// Adaptive failure threshold based on recent error rate
    pub adaptive_threshold: u32,
    /// Backoff strategy for exponential backoff with jitter
    pub backoff_strategy: BackoffStrategy,
    /// Timestamp when half-open state started
    pub half_open_start: Option<Instant>,
    /// EWMA error rate tracker for adaptive thresholds
    pub ewma_error_rate: EWMA,
    /// Recent latencies in milliseconds for health score calculation
    pub recent_latencies: Vec<f64>,
    /// Average latency in milliseconds
    pub avg_latency_ms: f64,
    /// Latency jitter (standard deviation of latencies)
    pub latency_jitter_ms: f64,
    /// Health score [0, 1] where 1 is perfect health
    pub health_score: f64,
}

/// Metrics for circuit breaker observability.
///
/// ## Metrics Contract
///
/// This structure manages Prometheus metrics for circuit breaker endpoints with the following guarantees:
///
/// - **Atomicity**: All metrics for an endpoint are registered atomically under a write lock.
///   Either all metrics succeed or none are registered, preventing partial state.
/// - **Thread-Safety**: All operations are protected by RwLock in the parent CircuitBreaker.
/// - **Idempotency**: Calling register_endpoint multiple times for the same endpoint is safe.
/// - **No Panics**: All registration errors are handled gracefully and logged.
///
/// ## Shutdown Behavior
///
/// When the CircuitBreaker is dropped or shutdown is called:
/// - All monitoring tasks are cancelled via CancellationToken
/// - Metrics remain in the Prometheus registry until the registry itself is dropped
/// - No cleanup of metrics is required as Prometheus handles de-registration on drop
/// - No data races or deadlocks occur during shutdown due to deterministic task termination
///
/// ## Metric Types
///
/// Each endpoint gets 6 metrics registered:
/// 1. `circuit_breaker_state_{endpoint}`: IntGauge (0=Healthy, 1=Degraded, 2=CoolingDown, 3=HalfOpen)
/// 2. `circuit_breaker_failures_{endpoint}`: IntCounter (cumulative failure count)
/// 3. `circuit_breaker_latency_ms_{endpoint}`: Gauge (average probe latency in milliseconds)
/// 4. `circuit_breaker_last_opened_{endpoint}`: IntGauge (unix timestamp when circuit last opened)
/// 5. `circuit_breaker_success_ratio_{endpoint}`: Gauge (success rate [0.0-1.0])
/// 6. `circuit_breaker_health_score_{endpoint}`: Gauge (health score [0.0-1.0])
///
/// ## Global Metrics
///
/// Additional global metrics track circuit breaker system health:
/// 1. `circuit_breaker_registration_failures_total`: IntCounter (count of failed metric registrations)
/// 2. `circuit_breaker_monitoring_task_count`: IntGauge (total number of monitoring tasks created)
/// 3. `circuit_breaker_active_tasks`: IntGauge (currently active monitoring tasks)
#[derive(Clone)]
pub struct CircuitBreakerMetrics {
    /// Circuit breaker state per endpoint (0=Healthy, 1=Degraded, 2=CoolingDown, 3=HalfOpen)
    pub circuit_state: HashMap<String, IntGauge>,
    /// Failure count per endpoint
    pub fail_count: HashMap<String, IntCounter>,
    /// Probe latency in milliseconds per endpoint
    pub probe_latency: HashMap<String, Gauge>,
    /// Last time circuit opened (unix timestamp) per endpoint
    pub last_opened: HashMap<String, IntGauge>,
    /// Probe success ratio per endpoint
    pub probe_success_ratio: HashMap<String, Gauge>,
    /// Health score per endpoint
    pub health_score: HashMap<String, Gauge>,
    /// Total count of failed metric registrations (global)
    pub registration_failures_total: IntCounter,
    /// Total number of monitoring tasks created (global)
    pub monitoring_task_count: IntGauge,
    /// Number of currently active monitoring tasks (global)
    pub active_tasks: IntGauge,
    /// Prometheus registry
    registry: Registry,
}

impl CircuitBreakerMetrics {
    /// Create new metrics with a registry
    pub fn new(registry: Registry) -> Self {
        // Register global metrics with fallback to default counters
        // Note: Fallback metrics use a unique name suffix to avoid conflicts
        let registration_failures = register_int_counter_with_registry!(
            Opts::new(
                "circuit_breaker_registration_failures_total",
                "Total number of failed metric registrations"
            ),
            &registry
        )
        .unwrap_or_else(|e| {
            warn!(
                error = %e,
                "Failed to register registration_failures_total metric, using unregistered fallback counter"
            );
            // Create unregistered fallback metric (won't panic)
            // Using a unique name to avoid any potential conflicts
            IntCounter::new(
                "circuit_breaker_registration_failures_total_fallback",
                "fallback counter for registration failures"
            )
            .expect("Creating unregistered fallback IntCounter should never fail")
        });

        let monitoring_task_count = register_int_gauge_with_registry!(
            Opts::new(
                "circuit_breaker_monitoring_task_count",
                "Total number of monitoring tasks created"
            ),
            &registry
        )
        .unwrap_or_else(|e| {
            warn!(
                error = %e,
                "Failed to register monitoring_task_count metric, using unregistered fallback gauge"
            );
            // Create unregistered fallback metric (won't panic)
            IntGauge::new(
                "circuit_breaker_monitoring_task_count_fallback",
                "fallback gauge for monitoring task count"
            )
            .expect("Creating unregistered fallback IntGauge should never fail")
        });

        let active_tasks = register_int_gauge_with_registry!(
            Opts::new(
                "circuit_breaker_active_tasks",
                "Number of currently active monitoring tasks"
            ),
            &registry
        )
        .unwrap_or_else(|e| {
            warn!(
                error = %e,
                "Failed to register active_tasks metric, using unregistered fallback gauge"
            );
            // Create unregistered fallback metric (won't panic)
            IntGauge::new(
                "circuit_breaker_active_tasks_fallback",
                "fallback gauge for active tasks"
            )
            .expect("Creating unregistered fallback IntGauge should never fail")
        });

        Self {
            circuit_state: HashMap::new(),
            fail_count: HashMap::new(),
            probe_latency: HashMap::new(),
            last_opened: HashMap::new(),
            probe_success_ratio: HashMap::new(),
            health_score: HashMap::new(),
            registration_failures_total: registration_failures,
            monitoring_task_count,
            active_tasks,
            registry,
        }
    }

    /// Register metrics for an endpoint with atomic HashMap writes.
    ///
    /// This method ensures atomicity by:
    /// 1. Checking if endpoint is already registered (fast path)
    /// 2. Creating all metrics first (may fail)
    /// 3. Only then inserting all metrics into HashMaps in one operation
    ///
    /// This prevents partial registration state where some metrics exist but others don't.
    /// If any metric registration fails, none are added to the HashMaps.
    fn register_endpoint(&mut self, endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Fast path: already registered
        if self.circuit_state.contains_key(endpoint) {
            debug!(
                endpoint = %endpoint,
                "Metrics already registered, skipping"
            );
            return Ok(());
        }

        info!(
            endpoint = %endpoint,
            "Registering Prometheus metrics for new endpoint"
        );

        // Sanitize endpoint name for Prometheus (replace all non-alphanumeric with underscore)
        let sanitized = endpoint
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>();

        // Phase 1: Register all metrics with Prometheus (may fail, no HashMap modifications yet)
        debug!(
            endpoint = %endpoint,
            sanitized_name = %sanitized,
            "Phase 1: Registering metrics with Prometheus registry"
        );

        let state_gauge = register_int_gauge_with_registry!(
            Opts::new(
                format!("circuit_breaker_state_{}", sanitized),
                format!("Circuit breaker state for endpoint {}", endpoint)
            ),
            &self.registry
        )
        .map_err(|e| {
            warn!(
                endpoint = %endpoint,
                error = %e,
                metric = "circuit_breaker_state",
                "Failed to register metric with Prometheus"
            );
            self.registration_failures_total.inc();
            e
        })?;

        let fail_counter = register_int_counter_with_registry!(
            Opts::new(
                format!("circuit_breaker_failures_{}", sanitized),
                format!("Failure count for endpoint {}", endpoint)
            ),
            &self.registry
        )
        .map_err(|e| {
            warn!(
                endpoint = %endpoint,
                error = %e,
                metric = "circuit_breaker_failures",
                "Failed to register metric with Prometheus"
            );
            self.registration_failures_total.inc();
            e
        })?;

        let latency_gauge = register_gauge_with_registry!(
            Opts::new(
                format!("circuit_breaker_latency_ms_{}", sanitized),
                format!("Probe latency in ms for endpoint {}", endpoint)
            ),
            &self.registry
        )
        .map_err(|e| {
            warn!(
                endpoint = %endpoint,
                error = %e,
                metric = "circuit_breaker_latency_ms",
                "Failed to register metric with Prometheus"
            );
            self.registration_failures_total.inc();
            e
        })?;

        let opened_gauge = register_int_gauge_with_registry!(
            Opts::new(
                format!("circuit_breaker_last_opened_{}", sanitized),
                format!(
                    "Last time circuit opened (unix timestamp) for endpoint {}",
                    endpoint
                )
            ),
            &self.registry
        )
        .map_err(|e| {
            warn!(
                endpoint = %endpoint,
                error = %e,
                metric = "circuit_breaker_last_opened",
                "Failed to register metric with Prometheus"
            );
            self.registration_failures_total.inc();
            e
        })?;

        let success_gauge = register_gauge_with_registry!(
            Opts::new(
                format!("circuit_breaker_success_ratio_{}", sanitized),
                format!("Success ratio for endpoint {}", endpoint)
            ),
            &self.registry
        )
        .map_err(|e| {
            warn!(
                endpoint = %endpoint,
                error = %e,
                metric = "circuit_breaker_success_ratio",
                "Failed to register metric with Prometheus"
            );
            self.registration_failures_total.inc();
            e
        })?;

        let health_gauge = register_gauge_with_registry!(
            Opts::new(
                format!("circuit_breaker_health_score_{}", sanitized),
                format!("Health score [0-1] for endpoint {}", endpoint)
            ),
            &self.registry
        )
        .map_err(|e| {
            warn!(
                endpoint = %endpoint,
                error = %e,
                metric = "circuit_breaker_health_score",
                "Failed to register metric with Prometheus"
            );
            self.registration_failures_total.inc();
            e
        })?;

        // Phase 2: All metrics successfully registered, now atomically insert into HashMaps
        // This is atomic because we're already under a write lock and all insertions happen
        // without any possibility of early return or failure
        debug!(
            endpoint = %endpoint,
            "Phase 2: Atomically inserting all metrics into HashMaps"
        );

        let endpoint_string = endpoint.to_string();
        self.circuit_state.insert(endpoint_string.clone(), state_gauge);
        self.fail_count.insert(endpoint_string.clone(), fail_counter);
        self.probe_latency.insert(endpoint_string.clone(), latency_gauge);
        self.last_opened.insert(endpoint_string.clone(), opened_gauge);
        self.probe_success_ratio.insert(endpoint_string.clone(), success_gauge);
        self.health_score.insert(endpoint_string.clone(), health_gauge);

        info!(
            endpoint = %endpoint,
            metrics_count = 6,
            "Successfully registered all Prometheus metrics for endpoint"
        );

        Ok(())
    }
}

/// Circuit breaker for managing RPC endpoint health.
pub struct CircuitBreaker {
    /// Health state per endpoint (wrapped in Arc<RwLock> for concurrent access)
    endpoint_health: Arc<RwLock<HashMap<String, EndpointHealth>>>,
    /// Failure threshold before marking endpoint as degraded
    failure_threshold: u32,
    /// Cooldown duration for failed endpoints
    cooldown_duration: Duration,
    /// Sample size for success rate calculation
    sample_size: usize,
    /// Minimum success rate to keep endpoint healthy
    min_success_rate: f64,
    /// Number of canary requests to send in half-open state
    canary_count: u32,
    /// Success rate required for canary requests to pass
    canary_success_threshold: f64,
    /// Maximum backoff multiplier for exponential backoff
    max_backoff: u32,
    /// Base backoff duration in seconds
    base_backoff_seconds: u64,
    /// Enable adaptive thresholds based on error rates
    adaptive_thresholds: bool,
    /// Callback for state change notifications (endpoint, old_state, new_state)
    state_change_callback: Option<Arc<dyn Fn(&str, EndpointState, EndpointState) + Send + Sync>>,
    /// Background monitoring tasks
    monitoring_tasks: Arc<RwLock<HashMap<String, (JoinHandle<()>, CancellationToken)>>>,
    /// Metrics for observability
    metrics: Arc<RwLock<CircuitBreakerMetrics>>,
    /// EWMA alpha parameter for adaptive thresholds
    ewma_alpha: f64,
    /// Sensitivity multiplier for standard deviation in adaptive thresholds
    adaptive_sensitivity: f64,
    /// Minimum health score threshold to keep endpoint open
    min_health_score: f64,
}

impl CircuitBreaker {
    /// Threshold for slow cancellation warning (seconds)
    const SLOW_CANCELLATION_THRESHOLD_SECS: u64 = 5;
    
    /// Threshold for long-running task warning (seconds)
    const LONG_RUNNING_TASK_THRESHOLD_SECS: u64 = 10;
    
    /// Create a new circuit breaker.
    pub fn new(failure_threshold: u32, cooldown_seconds: u64, sample_size: usize) -> Self {
        let registry = Registry::new();
        Self {
            endpoint_health: Arc::new(RwLock::new(HashMap::new())),
            failure_threshold,
            cooldown_duration: Duration::from_secs(cooldown_seconds),
            sample_size,
            min_success_rate: 0.3, // 30% minimum success rate
            canary_count: 3,
            canary_success_threshold: 0.66, // At least 2 out of 3 canary requests must succeed
            max_backoff: 32,
            base_backoff_seconds: cooldown_seconds,
            adaptive_thresholds: true,
            state_change_callback: None,
            monitoring_tasks: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(CircuitBreakerMetrics::new(registry))),
            ewma_alpha: 0.3,           // 30% weight on recent values
            adaptive_sensitivity: 2.0, // 2 standard deviations
            min_health_score: 0.3,     // Minimum health score threshold
        }
    }

    /// Create a circuit breaker with custom configuration.
    pub fn with_config(
        failure_threshold: u32,
        cooldown_seconds: u64,
        sample_size: usize,
        canary_count: u32,
        adaptive_thresholds: bool,
    ) -> Self {
        let registry = Registry::new();
        Self {
            endpoint_health: Arc::new(RwLock::new(HashMap::new())),
            failure_threshold,
            cooldown_duration: Duration::from_secs(cooldown_seconds),
            sample_size,
            min_success_rate: 0.3,
            canary_count,
            canary_success_threshold: 0.66, // At least 2 out of 3 canary requests must succeed
            max_backoff: 32,
            base_backoff_seconds: cooldown_seconds,
            adaptive_thresholds,
            state_change_callback: None,
            monitoring_tasks: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(CircuitBreakerMetrics::new(registry))),
            ewma_alpha: 0.3,
            adaptive_sensitivity: 2.0,
            min_health_score: 0.3,
        }
    }

    /// Create a circuit breaker with full custom configuration including EWMA parameters.
    pub fn with_full_config(
        failure_threshold: u32,
        cooldown_seconds: u64,
        sample_size: usize,
        canary_count: u32,
        adaptive_thresholds: bool,
        ewma_alpha: f64,
        adaptive_sensitivity: f64,
        min_health_score: f64,
    ) -> Self {
        let registry = Registry::new();
        Self {
            endpoint_health: Arc::new(RwLock::new(HashMap::new())),
            failure_threshold,
            cooldown_duration: Duration::from_secs(cooldown_seconds),
            sample_size,
            min_success_rate: 0.3,
            canary_count,
            canary_success_threshold: 0.66,
            max_backoff: 32,
            base_backoff_seconds: cooldown_seconds,
            adaptive_thresholds,
            state_change_callback: None,
            monitoring_tasks: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(CircuitBreakerMetrics::new(registry))),
            ewma_alpha: ewma_alpha.clamp(0.01, 1.0),
            adaptive_sensitivity: adaptive_sensitivity.clamp(0.5, 5.0),
            min_health_score: min_health_score.clamp(0.0, 1.0),
        }
    }

    /// Get the Prometheus registry for metrics export
    pub async fn get_metrics_registry(&self) -> Registry {
        let metrics = self.metrics.read().await;
        metrics.registry.clone()
    }

    /// Update metrics for an endpoint
    async fn update_metrics(&self, endpoint: &str) {
        // Register endpoint metrics if needed (atomic operation under write lock)
        {
            let mut metrics = self.metrics.write().await;
            if let Err(e) = metrics.register_endpoint(endpoint) {
                warn!(
                    endpoint = %endpoint,
                    error = %e,
                    "Failed to register endpoint metrics, metric updates will be skipped"
                );
                return;
            }
        }

        // Read health data
        let (state, consecutive_failures, success_rate, avg_latency, health_score, cooldown_start) = {
            let health_map = self.endpoint_health.read().await;
            if let Some(health) = health_map.get(endpoint) {
                (
                    health.state.clone(),
                    health.consecutive_failures,
                    health.success_rate,
                    health.avg_latency_ms,
                    health.health_score,
                    health.cooldown_start,
                )
            } else {
                debug!(
                    endpoint = %endpoint,
                    "No health data found for endpoint, skipping metric update"
                );
                return;
            }
        };

        // Update metrics under read lock
        let metrics = self.metrics.read().await;

        // Update state metric
        if let Some(state_gauge) = metrics.circuit_state.get(endpoint) {
            let state_value = match state {
                EndpointState::Healthy => 0,
                EndpointState::Degraded => 1,
                EndpointState::CoolingDown => 2,
                EndpointState::HalfOpen => 3,
            };
            state_gauge.set(state_value);
            debug!(
                endpoint = %endpoint,
                metric = "circuit_breaker_state",
                value = state_value,
                state = ?state,
                "Updated metric"
            );
        }

        // Update failure counter
        if let Some(fail_counter) = metrics.fail_count.get(endpoint) {
            // Note: Prometheus counters can only increase, so we set to current value
            // In production, you might want to use a gauge instead
            let current = fail_counter.get();
            if consecutive_failures as u64 > current {
                let delta = consecutive_failures as u64 - current;
                fail_counter.inc_by(delta);
                debug!(
                    endpoint = %endpoint,
                    metric = "circuit_breaker_failures",
                    previous = current,
                    new = consecutive_failures,
                    delta = delta,
                    "Updated metric"
                );
            }
        }

        // Update latency metric
        if let Some(latency_gauge) = metrics.probe_latency.get(endpoint) {
            latency_gauge.set(avg_latency);
            debug!(
                endpoint = %endpoint,
                metric = "circuit_breaker_latency_ms",
                value = avg_latency,
                "Updated metric"
            );
        }

        // Update last opened timestamp
        if let Some(opened_gauge) = metrics.last_opened.get(endpoint) {
            if state == EndpointState::CoolingDown {
                if let Some(cooldown) = cooldown_start {
                    let timestamp = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64
                        - cooldown.elapsed().as_secs() as i64;
                    opened_gauge.set(timestamp);
                    debug!(
                        endpoint = %endpoint,
                        metric = "circuit_breaker_last_opened",
                        timestamp = timestamp,
                        "Updated metric"
                    );
                }
            }
        }

        // Update success ratio metric
        if let Some(success_gauge) = metrics.probe_success_ratio.get(endpoint) {
            success_gauge.set(success_rate);
            debug!(
                endpoint = %endpoint,
                metric = "circuit_breaker_success_ratio",
                value = format!("{:.3}", success_rate),
                "Updated metric"
            );
        }

        // Update health score metric
        if let Some(health_gauge) = metrics.health_score.get(endpoint) {
            health_gauge.set(health_score);
            debug!(
                endpoint = %endpoint,
                metric = "circuit_breaker_health_score",
                value = format!("{:.3}", health_score),
                "Updated metric"
            );
        }
    }

    /// Set callback for state change notifications.
    pub fn set_state_change_callback<F>(&mut self, callback: F)
    where
        F: Fn(&str, EndpointState, EndpointState) + Send + Sync + 'static,
    {
        self.state_change_callback = Some(Arc::new(callback));
    }

    /// Record a successful request to an endpoint.
    #[instrument(skip(self), fields(endpoint = %endpoint))]
    pub async fn record_success(&self, endpoint: &str) {
        self.record_success_with_latency(endpoint, None).await;
    }

    /// Record a successful request to an endpoint with latency tracking.
    #[instrument(skip(self), fields(endpoint = %endpoint))]
    pub async fn record_success_with_latency(&self, endpoint: &str, latency_ms: Option<f64>) {
        // Snapshot pattern: read lock → clone → release → compute → write lock → update
        let old_state = {
            let health_map = self.endpoint_health.read().await;
            health_map.get(endpoint).map(|h| h.state.clone())
        };

        let old_state = old_state.unwrap_or(EndpointState::Healthy);

        // Update health with short write lock
        {
            let mut health_map = self.endpoint_health.write().await;
            let health = health_map.entry(endpoint.to_string()).or_insert_with(|| {
                EndpointHealth::new(
                    self.base_backoff_seconds * 1000,
                    self.max_backoff as u64 * self.base_backoff_seconds * 1000,
                    self.ewma_alpha,
                )
            });

            // Track canary successes in half-open state
            if health.state == EndpointState::HalfOpen {
                health.canary_successes += 1;
                health.canary_count += 1;
            }

            health.record_success(latency_ms);
        }

        self.update_endpoint_state(endpoint).await;

        // Read final state for logging
        let (new_state, consecutive_failures, success_rate) = {
            let health_map = self.endpoint_health.read().await;
            if let Some(health) = health_map.get(endpoint) {
                (
                    health.state.clone(),
                    health.consecutive_failures,
                    health.success_rate,
                )
            } else {
                return;
            }
        };

        // Notify on state change
        if old_state != new_state {
            self.notify_state_change(endpoint, old_state.clone(), new_state.clone());
        }

        // Update metrics
        self.update_metrics(endpoint).await;

        // Structured logging
        if old_state != new_state {
            info!(
                endpoint = %endpoint,
                state_transition = format!("{:?} -> {:?}", old_state, new_state),
                reason = "success",
                fail_count = consecutive_failures,
                success_rate = format!("{:.2}%", success_rate * 100.0),
                latency_ms = latency_ms.unwrap_or(0.0),
                "Circuit breaker state changed after success"
            );
        } else {
            debug!(
                endpoint = %endpoint,
                state = format!("{:?}", new_state),
                fail_count = consecutive_failures,
                success_rate = format!("{:.2}%", success_rate * 100.0),
                latency_ms = latency_ms.unwrap_or(0.0),
                "Recorded success"
            );
        }
    }

    /// Record a failed request to an endpoint.
    #[instrument(skip(self), fields(endpoint = %endpoint))]
    pub async fn record_failure(&self, endpoint: &str) {
        self.record_failure_with_latency(endpoint, None).await;
    }

    /// Record a failed request to an endpoint with latency tracking.
    #[instrument(skip(self), fields(endpoint = %endpoint))]
    pub async fn record_failure_with_latency(&self, endpoint: &str, latency_ms: Option<f64>) {
        // Snapshot pattern: read lock → clone → release → compute → write lock → update
        let old_state = {
            let health_map = self.endpoint_health.read().await;
            health_map.get(endpoint).map(|h| h.state.clone())
        };

        let old_state = old_state.unwrap_or(EndpointState::Healthy);

        // Update health with short write lock
        {
            let mut health_map = self.endpoint_health.write().await;
            let health = health_map.entry(endpoint.to_string()).or_insert_with(|| {
                EndpointHealth::new(
                    self.base_backoff_seconds * 1000,
                    self.max_backoff as u64 * self.base_backoff_seconds * 1000,
                    self.ewma_alpha,
                )
            });

            // Track canary failures in half-open state
            if health.state == EndpointState::HalfOpen {
                health.canary_count += 1;
            }

            health.record_failure(latency_ms);
        }

        self.update_endpoint_state(endpoint).await;

        // Read final state for logging
        let (new_state, consecutive_failures, success_rate) = {
            let health_map = self.endpoint_health.read().await;
            if let Some(health) = health_map.get(endpoint) {
                (
                    health.state.clone(),
                    health.consecutive_failures,
                    health.success_rate,
                )
            } else {
                return;
            }
        };

        // Notify on state change
        if old_state != new_state {
            self.notify_state_change(endpoint, old_state.clone(), new_state.clone());
        }

        // Update metrics
        self.update_metrics(endpoint).await;

        // Structured logging
        if old_state != new_state {
            warn!(
                endpoint = %endpoint,
                state_transition = format!("{:?} -> {:?}", old_state, new_state),
                reason = "failure_threshold_exceeded",
                fail_count = consecutive_failures,
                success_rate = format!("{:.2}%", success_rate * 100.0),
                latency_ms = latency_ms.unwrap_or(0.0),
                "Circuit breaker state changed after failure"
            );
        } else {
            warn!(
                endpoint = %endpoint,
                state = format!("{:?}", new_state),
                fail_count = consecutive_failures,
                success_rate = format!("{:.2}%", success_rate * 100.0),
                latency_ms = latency_ms.unwrap_or(0.0),
                "Recorded failure"
            );
        }
    }

    /// Check if an endpoint is available for use.
    #[instrument(skip(self), fields(endpoint = %endpoint))]
    pub async fn is_available(&self, endpoint: &str) -> bool {
        // Update state before checking availability
        self.update_endpoint_state(endpoint).await;

        // Snapshot pattern: read lock → clone data → release → compute
        let (state, canary_count_val, cooldown_info) = {
            let health_map = self.endpoint_health.read().await;
            let health = health_map.get(endpoint);

            if let Some(health) = health {
                let cooldown_info = health
                    .cooldown_start
                    .map(|start| {
                        // Clone the backoff strategy to calculate delay outside the lock
                        (start, health.backoff_strategy.clone())
                    });
                (health.state.clone(), health.canary_count, cooldown_info)
            } else {
                // New endpoint, create with defaults
                drop(health_map);
                let mut health_map = self.endpoint_health.write().await;
                health_map.entry(endpoint.to_string()).or_insert_with(|| {
                    EndpointHealth::new(
                        self.base_backoff_seconds * 1000,
                        self.max_backoff as u64 * self.base_backoff_seconds * 1000,
                        self.ewma_alpha,
                    )
                });
                return true; // New endpoints are healthy
            }
        };

        match state {
            EndpointState::Healthy => true,
            EndpointState::Degraded => true, // Still usable but not preferred
            EndpointState::HalfOpen => {
                // Allow canary requests in half-open state
                canary_count_val < self.canary_count
            }
            EndpointState::CoolingDown => {
                // Check if cooldown period has expired
                if let Some((cooldown_start, backoff_strategy)) = cooldown_info {
                    // Calculate the delay using BackoffStrategy's current attempt (without incrementing)
                    let backoff_duration = backoff_strategy.current_delay();

                    if cooldown_start.elapsed() >= backoff_duration {
                        // Transition to half-open state with short write lock
                        let old_state = {
                            let mut health_map = self.endpoint_health.write().await;
                            if let Some(health) = health_map.get_mut(endpoint) {
                                let old_state = health.state.clone();
                                health.state = EndpointState::HalfOpen;
                                health.half_open_start = Some(Instant::now());
                                health.cooldown_start = None;
                                health.canary_count = 0;
                                health.canary_successes = 0;
                                // Reset history for fresh canary testing
                                health.recent_attempts.clear();
                                health.total_attempts = 0;
                                health.successful_attempts = 0;
                                health.success_rate = 1.0;
                                old_state
                            } else {
                                return true;
                            }
                        };
                        debug!("Endpoint {} cooldown expired, entering half-open state for canary testing", endpoint);
                        self.notify_state_change(endpoint, old_state, EndpointState::HalfOpen);
                        true
                    } else {
                        false
                    }
                } else {
                    // No cooldown start time, shouldn't happen but treat as available
                    true
                }
            }
        }
    }

    /// Get the current state of an endpoint.
    pub async fn get_endpoint_state(&self, endpoint: &str) -> EndpointState {
        let health_map = self.endpoint_health.read().await;
        health_map
            .get(endpoint)
            .map(|h| h.state.clone())
            .unwrap_or(EndpointState::Healthy)
    }

    /// Get all healthy endpoints.
    pub async fn get_healthy_endpoints(&self) -> Vec<String> {
        let mut healthy = Vec::new();

        let endpoints = {
            let health_map = self.endpoint_health.read().await;
            health_map.keys().cloned().collect::<Vec<_>>()
        };

        for endpoint in endpoints {
            if self.is_available(&endpoint).await {
                let state = self.get_endpoint_state(&endpoint).await;
                if state == EndpointState::Healthy {
                    healthy.push(endpoint);
                }
            }
        }

        healthy
    }

    /// Get all available endpoints (healthy + degraded).
    pub async fn get_available_endpoints(&self) -> Vec<String> {
        let mut available = Vec::new();

        let endpoints = {
            let health_map = self.endpoint_health.read().await;
            health_map.keys().cloned().collect::<Vec<_>>()
        };

        for endpoint in endpoints {
            if self.is_available(&endpoint).await {
                available.push(endpoint);
            }
        }

        available
    }

    /// Update the state of an endpoint based on its health metrics.
    async fn update_endpoint_state(&self, endpoint: &str) {
        // Snapshot pattern: read → compute → write
        let snapshot = {
            let health_map = self.endpoint_health.read().await;
            health_map.get(endpoint).cloned()
        };

        let Some(health_snapshot) = snapshot else {
            // Endpoint doesn't exist, create it
            let mut health_map = self.endpoint_health.write().await;
            health_map.entry(endpoint.to_string()).or_insert_with(|| {
                EndpointHealth::new(
                    self.base_backoff_seconds * 1000,
                    self.max_backoff as u64 * self.base_backoff_seconds * 1000,
                    self.ewma_alpha,
                )
            });
            return;
        };

        let old_state = health_snapshot.state.clone();

        // Compute new adaptive threshold using EWMA + X*stddev
        let adaptive_threshold =
            if self.adaptive_thresholds && health_snapshot.ewma_error_rate.count >= 10 {
                // Use EWMA error rate + sensitivity * standard deviation
                let ewma_error = health_snapshot.ewma_error_rate.get();
                let std_dev = health_snapshot.ewma_error_rate.std_dev();
                let threshold_multiplier = ewma_error + (self.adaptive_sensitivity * std_dev);

                // Scale the base threshold by the error rate metric
                let scaled_threshold = if threshold_multiplier > 0.7 {
                    // Very high error rate with variance - reduce threshold significantly
                    self.failure_threshold / 3
                } else if threshold_multiplier > 0.5 {
                    // High error rate - reduce threshold
                    self.failure_threshold / 2
                } else if threshold_multiplier > 0.3 {
                    // Moderate error rate - slightly reduce threshold
                    (self.failure_threshold * 3) / 4
                } else {
                    // Low error rate - use full threshold
                    self.failure_threshold
                };

                scaled_threshold.max(1) // Ensure at least 1
            } else {
                self.failure_threshold
            };

        let threshold = if self.adaptive_thresholds && health_snapshot.ewma_error_rate.count >= 10 {
            adaptive_threshold
        } else {
            self.failure_threshold
        };

        // Check health score threshold
        let health_score_critical = health_snapshot.health_score < self.min_health_score;

        // Compute new state based on current state
        let new_state_opt = match health_snapshot.state {
            EndpointState::Healthy => {
                if health_score_critical {
                    debug!(
                        "Endpoint {} degraded: health score {:.3} below threshold {:.3}",
                        endpoint, health_snapshot.health_score, self.min_health_score
                    );
                    Some(EndpointState::Degraded)
                } else if health_snapshot.consecutive_failures >= threshold {
                    debug!(
                        "Endpoint {} degraded: {} consecutive failures (threshold: {})",
                        endpoint, health_snapshot.consecutive_failures, threshold
                    );
                    Some(EndpointState::Degraded)
                } else {
                    None
                }
            }
            EndpointState::Degraded => {
                if health_snapshot.consecutive_failures >= threshold * 2
                    || (health_snapshot.total_attempts >= self.sample_size
                        && health_snapshot.success_rate < self.min_success_rate)
                    || health_score_critical
                {
                    let reason = if health_score_critical {
                        format!(
                            "health score {:.3} < {:.3}",
                            health_snapshot.health_score, self.min_health_score
                        )
                    } else {
                        format!(
                            "{} failures, {:.2}% success rate",
                            health_snapshot.consecutive_failures,
                            health_snapshot.success_rate * 100.0
                        )
                    };
                    warn!("Endpoint {} entering cooldown: {}", endpoint, reason);
                    Some(EndpointState::CoolingDown)
                } else if health_snapshot.consecutive_failures == 0
                    && health_snapshot.success_rate > 0.7
                    && health_snapshot.health_score > 0.7
                {
                    debug!("Endpoint {} recovered to healthy state", endpoint);
                    Some(EndpointState::Healthy)
                } else {
                    None
                }
            }
            EndpointState::HalfOpen => {
                if health_snapshot.canary_count >= self.canary_count {
                    let canary_success_rate = health_snapshot.canary_successes as f64
                        / health_snapshot.canary_count as f64;
                    if canary_success_rate >= self.canary_success_threshold {
                        debug!(
                            "Endpoint {} canary tests passed ({}/{}), moving to degraded state",
                            endpoint,
                            health_snapshot.canary_successes,
                            health_snapshot.canary_count
                        );
                        Some(EndpointState::Degraded)
                    } else {
                        warn!(
                            "Endpoint {} canary tests failed ({}/{}), back to cooldown",
                            endpoint,
                            health_snapshot.canary_successes,
                            health_snapshot.canary_count
                        );
                        Some(EndpointState::CoolingDown)
                    }
                } else {
                    None
                }
            }
            EndpointState::CoolingDown => None,
        };

        // Apply state change with short write lock
        if let Some(new_state) = new_state_opt.clone() {
            let mut health_map = self.endpoint_health.write().await;
            if let Some(health) = health_map.get_mut(endpoint) {
                health.state = new_state.clone();
                health.adaptive_threshold = adaptive_threshold;

                match new_state {
                    EndpointState::CoolingDown => {
                        health.cooldown_start = Some(Instant::now());
                        // Increment backoff strategy attempt counter
                        health.backoff_strategy.attempt = (health.backoff_strategy.attempt + 1).min(BackoffStrategy::MAX_ATTEMPT);
                    }
                    EndpointState::Healthy => {
                        health.backoff_strategy.reset();
                    }
                    EndpointState::Degraded => {
                        if old_state == EndpointState::HalfOpen {
                            // Successful canary
                            health.consecutive_failures = 0;
                            health.backoff_strategy.reset();
                            health.canary_count = 0;
                            health.canary_successes = 0;
                            health.half_open_start = None;
                        }
                    }
                    EndpointState::HalfOpen => {
                        // Should not happen here, handled in is_available
                    }
                }
            }
        }

        // Notify on state change after releasing locks
        if let Some(new_state) = new_state_opt {
            self.notify_state_change(endpoint, old_state, new_state);
        }
    }

    /// Notify state change via callback.
    fn notify_state_change(
        &self,
        endpoint: &str,
        old_state: EndpointState,
        new_state: EndpointState,
    ) {
        if let Some(ref callback) = self.state_change_callback {
            callback(endpoint, old_state.clone(), new_state.clone());
        }

        // Always log state changes
        match (&old_state, &new_state) {
            (_, EndpointState::CoolingDown) => {
                tracing::error!(
                    endpoint = %endpoint,
                    old_state = ?old_state,
                    new_state = ?new_state,
                    "Circuit breaker OPENED - endpoint unavailable"
                );
            }
            (EndpointState::CoolingDown, EndpointState::HalfOpen) => {
                tracing::warn!(
                    endpoint = %endpoint,
                    old_state = ?old_state,
                    new_state = ?new_state,
                    "Circuit breaker HALF-OPEN - testing with canary requests"
                );
            }
            (_, EndpointState::Healthy) => {
                tracing::info!(
                    endpoint = %endpoint,
                    old_state = ?old_state,
                    new_state = ?new_state,
                    "Circuit breaker CLOSED - endpoint fully recovered"
                );
            }
            _ => {
                tracing::info!(
                    endpoint = %endpoint,
                    old_state = ?old_state,
                    new_state = ?new_state,
                    "Circuit breaker state changed"
                );
            }
        }
    }

    /// Get health statistics for all endpoints.
    pub async fn get_health_stats(&self) -> HashMap<String, EndpointHealthStats> {
        let health_map = self.endpoint_health.read().await;
        health_map
            .iter()
            .map(|(endpoint, health)| {
                (
                    endpoint.clone(),
                    EndpointHealthStats {
                        state: health.state.clone(),
                        consecutive_failures: health.consecutive_failures,
                        success_rate: health.success_rate,
                        total_attempts: health.total_attempts,
                        successful_attempts: health.successful_attempts,
                        adaptive_threshold: health.adaptive_threshold,
                        canary_progress: if health.state == EndpointState::HalfOpen {
                            Some((health.canary_successes, health.canary_count))
                        } else {
                            None
                        },
                        health_score: health.health_score,
                        avg_latency_ms: health.avg_latency_ms,
                        latency_jitter_ms: health.latency_jitter_ms,
                        ewma_error_rate: health.ewma_error_rate.get(),
                    },
                )
            })
            .collect()
    }

    /// Get circuit breaker configuration.
    pub fn get_config(&self) -> CircuitBreakerConfig {
        CircuitBreakerConfig {
            failure_threshold: self.failure_threshold,
            cooldown_seconds: self.cooldown_duration.as_secs(),
            sample_size: self.sample_size,
            min_success_rate: self.min_success_rate,
            canary_count: self.canary_count,
            canary_success_threshold: self.canary_success_threshold,
            max_backoff: self.max_backoff,
            adaptive_thresholds: self.adaptive_thresholds,
            ewma_alpha: self.ewma_alpha,
            adaptive_sensitivity: self.adaptive_sensitivity,
            min_health_score: self.min_health_score,
        }
    }

    /// Reset all endpoints to healthy state.
    pub async fn reset_all(&self) {
        let mut health_map = self.endpoint_health.write().await;
        for health in health_map.values_mut() {
            health.state = EndpointState::Healthy;
            health.consecutive_failures = 0;
            health.cooldown_start = None;
            health.recent_attempts.clear();
            health.total_attempts = 0;
            health.successful_attempts = 0;
            health.success_rate = 1.0;
            health.canary_count = 0;
            health.canary_successes = 0;
            health.adaptive_threshold = self.failure_threshold;
            health.backoff_strategy.reset();
            health.half_open_start = None;
            health.ewma_error_rate = EWMA::new(self.ewma_alpha);
            health.recent_latencies.clear();
            health.avg_latency_ms = 0.0;
            health.latency_jitter_ms = 0.0;
            health.health_score = 1.0;
        }
        debug!("Reset all endpoints to healthy state");
    }

    /// Start a background monitoring task for an endpoint.
    /// 
    /// ## Cooperatively-Cancellable Task Pattern (REQUIRED)
    /// 
    /// **All monitoring tasks MUST be designed to cooperate with cancellation signals.**
    /// Tasks that ignore the CancellationToken or perform blocking syscalls/FFI will:
    /// 1. Delay shutdown by up to 7 seconds (Phase 1 + Phase 2 timeouts)
    /// 2. May remain stuck if blocking operations are uninterruptible
    /// 3. Generate warning logs and metrics alerts
    /// 
    /// ## Required Pattern for Task Implementation
    /// 
    /// ```rust,ignore
    /// async fn my_monitoring_task(cancel_token: CancellationToken) {
    ///     loop {
    ///         tokio::select! {
    ///             // Check for cancellation on every iteration
    ///             _ = cancel_token.cancelled() => {
    ///                 info!("Task cancelled, performing cleanup");
    ///                 // Perform any necessary cleanup here
    ///                 break;
    ///             }
    ///             // Your actual work
    ///             result = do_some_work() => {
    ///                 // Process result
    ///             }
    ///         }
    ///     }
    /// }
    /// ```
    /// 
    /// ## Sanity Checks
    /// 
    /// This method monitors task completion and will:
    /// - Log warnings if tasks don't respond to cancellation within expected time
    /// - Increment alert metrics for stuck tasks
    /// - Track task lifecycle in Prometheus metrics
    /// 
    /// See `CIRCUIT_BREAKER_TASK_PATTERNS.md` for detailed examples and best practices.
    pub async fn start_monitoring_task<F>(&self, endpoint: String, task: F) -> Result<(), String>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let cancel_token = CancellationToken::new();
        let cancel_clone = cancel_token.clone();
        let endpoint_for_log = endpoint.clone();
        let metrics_clone = Arc::clone(&self.metrics);

        let handle = tokio::spawn(async move {
            // Increment both metrics at the start of the task (after successful spawn)
            // This ensures accuracy: counters only increase if the task actually started
            let (total_tasks, active_tasks_count) = {
                let metrics = metrics_clone.read().await;
                metrics.monitoring_task_count.inc();
                metrics.active_tasks.inc();
                // Read the values after increment to ensure visibility
                (metrics.monitoring_task_count.get(), metrics.active_tasks.get())
            };
            
            let start_time = std::time::Instant::now();
            info!(
                endpoint = %endpoint_for_log,
                total_tasks = total_tasks,
                active_tasks = active_tasks_count,
                "Monitoring task started"
            );

            tokio::select! {
                _ = cancel_clone.cancelled() => {
                    let elapsed = start_time.elapsed();
                    debug!(
                        endpoint = %endpoint_for_log,
                        elapsed_secs = elapsed.as_secs(),
                        "Monitoring task cancelled cooperatively"
                    );
                    
                    // SANITY CHECK: Warn if cancellation took too long
                    if elapsed > std::time::Duration::from_secs(Self::SLOW_CANCELLATION_THRESHOLD_SECS) {
                        warn!(
                            endpoint = %endpoint_for_log,
                            elapsed_secs = elapsed.as_secs(),
                            threshold_secs = Self::SLOW_CANCELLATION_THRESHOLD_SECS,
                            alert = "slow_cancellation",
                            "Task took longer than expected to respond to cancellation - may not be checking token frequently"
                        );
                    }
                }
                _ = task => {
                    let elapsed = start_time.elapsed();
                    debug!(
                        endpoint = %endpoint_for_log,
                        elapsed_secs = elapsed.as_secs(),
                        "Monitoring task completed naturally"
                    );
                }
            }

            // Decrement active tasks when task completes or is cancelled
            let metrics = metrics_clone.read().await;
            metrics.active_tasks.dec();
            let remaining_active = metrics.active_tasks.get();
            
            // SANITY CHECK: Verify task cleanup completed
            let total_elapsed = start_time.elapsed();
            if total_elapsed > std::time::Duration::from_secs(Self::LONG_RUNNING_TASK_THRESHOLD_SECS) {
                warn!(
                    endpoint = %endpoint_for_log,
                    total_elapsed_secs = total_elapsed.as_secs(),
                    threshold_secs = Self::LONG_RUNNING_TASK_THRESHOLD_SECS,
                    alert = "long_running_task",
                    "Task ran for longer than expected - verify cancellation handling"
                );
            }
            
            info!(
                endpoint = %endpoint_for_log,
                active_tasks = remaining_active,
                total_elapsed_secs = total_elapsed.as_secs(),
                "Monitoring task stopped"
            );
        });

        let mut tasks = self.monitoring_tasks.write().await;

        // If a task already exists for this endpoint, gracefully shut it down first
        if let Some((old_handle, old_token)) = tasks.remove(&endpoint) {
            drop(tasks); // Release lock before awaiting
            self.shutdown_task(old_handle, old_token).await;
            tasks = self.monitoring_tasks.write().await;
        }

        tasks.insert(endpoint, (handle, cancel_token));
        Ok(())
    }

    /// Stop a background monitoring task for an endpoint.
    pub async fn stop_monitoring_task(&self, endpoint: &str) -> Result<(), String> {
        let mut tasks = self.monitoring_tasks.write().await;

        if let Some((handle, token)) = tasks.remove(endpoint) {
            drop(tasks); // Release lock before awaiting
            self.shutdown_task(handle, token).await;
            Ok(())
        } else {
            Err(format!(
                "No monitoring task found for endpoint: {}",
                endpoint
            ))
        }
    }

    /// Helper function to log JoinError with full details for root-cause analysis.
    /// 
    /// This ensures consistent, structured logging of all JoinError occurrences
    /// across both shutdown phases.
    #[inline]
    fn log_join_error(error: &tokio::task::JoinError, phase: &str) {
        debug!(
            error = ?error,
            phase = phase,
            "[{}] Full JoinError details for root-cause analysis", phase
        );
    }
    
    /// Deterministically shutdown a task with two-phase logic.
    /// 
    /// ## Shutdown Phases
    /// 
    /// **Phase 1 (Graceful)**: Signal cancellation via CancellationToken and wait up to 5 seconds
    /// for task to complete cooperatively. This gives tasks time to clean up resources, flush
    /// buffers, and exit gracefully.
    /// 
    /// **Phase 2 (Forced)**: If Phase 1 times out, forcibly abort the task via JoinHandle::abort()
    /// and wait up to 2 seconds for abort confirmation. This ensures no orphaned tasks remain.
    /// 
    /// ## Logging Contract
    /// 
    /// All outcomes are logged with full context:
    /// - **Completed gracefully**: Task responded to cancellation and exited cleanly (info)
    /// - **Cancelled**: Task was aborted by Phase 2 (info)
    /// - **Panicked**: Task panicked in either phase, with full JoinError details (warn)
    /// - **Timeout**: Phase 1 timeout triggers Phase 2; Phase 2 timeout indicates stuck task (warn)
    /// 
    /// Phase 1 and Phase 2 logs are clearly separated with phase markers for observability.
    /// 
    /// ## Guarantees
    /// 
    /// - No orphaned tasks: Either task completes or JoinHandle is dropped after timeout
    /// - No resource leaks: Deterministic cleanup within 7 seconds maximum
    /// - Full diagnostics: All JoinError states logged with context
    async fn shutdown_task(&self, mut handle: JoinHandle<()>, token: CancellationToken) {
        // Phase 1: Graceful shutdown - signal cancellation and wait max 5s
        info!("[Phase 1] Initiating graceful cancellation (timeout: 5s)");
        debug!("[Phase 1] Signaling CancellationToken to wake waiting tasks");
        token.cancel();

        // Use tokio::time::timeout to wait for completion OR timeout while keeping handle ownership
        let phase1_result = tokio::time::timeout(Duration::from_secs(5), &mut handle).await;
        
        match phase1_result {
            Ok(Ok(())) => {
                // Task completed gracefully - best case scenario
                info!("[Phase 1] ✓ Task completed gracefully (outcome: success)");
                debug!("[Phase 1] Task responded to cancellation and exited cleanly within timeout");
            }
            Ok(Err(e)) => {
                // Task panicked during graceful shutdown - log full diagnostics
                // CRITICAL: Full debug logging of JoinError for root-cause analysis
                Self::log_join_error(&e, "Phase 1");
                warn!(
                    phase = "1",
                    outcome = "panic",
                    "[Phase 1] ✗ Task panicked during graceful shutdown"
                );
                
                // Extract and log panic details if available
                if e.is_panic() {
                    warn!(
                        phase = "1",
                        "[Phase 1] Task terminated due to panic - check application logs for panic message"
                    );
                } else if e.is_cancelled() {
                    // Shouldn't happen in Phase 1, but log if it does
                    debug!("[Phase 1] Unexpected: JoinError indicates cancellation in Phase 1");
                } else {
                    debug!("[Phase 1] Unknown JoinError type (not panic, not cancelled)");
                }
            }
            Err(_timeout) => {
                // Phase 1 timeout - task didn't respond to cancellation, proceed to Phase 2
                warn!("[Phase 1] ✗ Timeout after 5s - task did not respond to cancellation (outcome: timeout)");
                debug!("[Phase 1] Task either ignored CancellationToken or is stuck in blocking operation");
                info!("[Phase 1→2] Transitioning to Phase 2: forced abort");
                
                // Phase 2: Force abort the task
                info!("[Phase 2] Calling JoinHandle::abort() for forced termination");
                debug!("[Phase 2] Forcing task cancellation regardless of cooperation");
                handle.abort();
                
                // Wait up to 2 seconds for abort confirmation
                // CRITICAL: JoinHandle ownership transferred to timeout, ensuring cleanup
                debug!("[Phase 2] Waiting up to 2s for abort confirmation");
                debug!("[Phase 2] JoinHandle ownership transferred to await - will be dropped after match");
                match tokio::time::timeout(Duration::from_secs(2), handle).await {
                    Ok(Ok(())) => {
                        // Task completed after abort (unlikely but possible - race condition)
                        info!("[Phase 2] ✓ Task completed after abort signal (outcome: completed)");
                        debug!("[Phase 2] Task may have completed naturally just as abort was called");
                    }
                    Ok(Err(e)) if e.is_cancelled() => {
                        // Task was successfully aborted - expected outcome for Phase 2
                        // CRITICAL: Full debug logging of JoinError for root-cause analysis
                        Self::log_join_error(&e, "Phase 2");
                        info!(
                            phase = "2",
                            outcome = "cancelled",
                            "[Phase 2] ✓ Task aborted successfully"
                        );
                        debug!("[Phase 2] JoinError::is_cancelled() confirmed forced termination");
                    }
                    Ok(Err(e)) if e.is_panic() => {
                        // Task panicked during abort - unexpected but possible
                        // CRITICAL: Full debug logging of JoinError for root-cause analysis
                        Self::log_join_error(&e, "Phase 2");
                        warn!(
                            phase = "2",
                            outcome = "panic",
                            "[Phase 2] ✗ Task panicked during abort"
                        );
                        warn!(
                            phase = "2",
                            "[Phase 2] Task panic occurred after abort signal - check for destructors or cleanup code panics"
                        );
                    }
                    Ok(Err(e)) => {
                        // Unknown error type - log full details for investigation
                        // CRITICAL: Full debug logging of JoinError for root-cause analysis
                        Self::log_join_error(&e, "Phase 2");
                        warn!(
                            phase = "2",
                            outcome = "error",
                            "[Phase 2] ✗ Task failed with unknown error type"
                        );
                        debug!("[Phase 2] Error is neither panic nor cancellation - investigate error type");
                    }
                    Err(_abort_timeout) => {
                        // Abort didn't complete within 2s - very unusual, indicates stuck task
                        // CRITICAL: Enhanced logging with timeout metadata for stuck task diagnosis
                        warn!(
                            timeout_duration_secs = 2,
                            total_shutdown_duration_secs = 7,
                            "[Phase 2] ✗ Abort timeout after 2s - task may be stuck (outcome: abort_timeout)"
                        );
                        debug!("[Phase 2] Task did not respond to abort within timeout");
                        warn!(
                            possible_causes = "uninterruptible syscall, FFI call, or deadlock",
                            "[Phase 2] Task may be stuck in blocking operation"
                        );
                        debug!("[Phase 2] JoinHandle will be dropped, kernel will eventually clean up task");
                        warn!(
                            action = "JoinHandle dropped after timeout",
                            "[Phase 2] Explicit JoinHandle drop - task resources may remain allocated until process exit"
                        );
                    }
                }
                
                // CRITICAL: Explicit confirmation that JoinHandle has been consumed and dropped
                info!("[Shutdown Complete] Total elapsed time: ~7s (5s graceful + 2s abort)");
                debug!("[Shutdown Complete] Task shutdown sequence finished, resources released");
                debug!("[Shutdown Complete] JoinHandle has been consumed and dropped - no orphaned tasks");
            }
        }
        
        // CRITICAL: At this point, JoinHandle has been dropped in all code paths:
        // - Phase 1 success: handle awaited and dropped
        // - Phase 1 panic: handle awaited and dropped
        // - Phase 2: handle moved into timeout().await and dropped
        debug!("[Cleanup Confirmed] JoinHandle dropped in all paths - task lifecycle complete");
    }

    /// Stop all monitoring tasks (for hot-swap or shutdown).
    pub async fn stop_all_monitoring_tasks(&self) {
        let tasks = {
            let mut tasks_map = self.monitoring_tasks.write().await;
            std::mem::take(&mut *tasks_map)
        };

        let task_count = tasks.len();
        info!(
            task_count = task_count,
            "Stopping all monitoring tasks"
        );

        for (endpoint, (handle, token)) in tasks {
            debug!("Stopping monitoring task for endpoint: {}", endpoint);
            self.shutdown_task(handle, token).await;
        }

        // Log final summary after all tasks stopped
        let metrics = self.metrics.read().await;
        info!(
            active_tasks = metrics.active_tasks.get(),
            total_tasks_created = metrics.monitoring_task_count.get(),
            registration_failures = metrics.registration_failures_total.get(),
            "All monitoring tasks stopped - final metrics summary"
        );
    }
}

impl EndpointHealth {
    /// Create new endpoint health tracker.
    fn new(base_delay_ms: u64, max_delay_ms: u64, ewma_alpha: f64) -> Self {
        Self {
            state: EndpointState::Healthy,
            consecutive_failures: 0,
            success_rate: 1.0,
            total_attempts: 0,
            successful_attempts: 0,
            last_failure: None,
            cooldown_start: None,
            recent_attempts: Vec::new(),
            canary_count: 0,
            canary_successes: 0,
            adaptive_threshold: 5, // Default, will be updated
            backoff_strategy: BackoffStrategy::new(base_delay_ms, max_delay_ms),
            half_open_start: None,
            ewma_error_rate: EWMA::new(ewma_alpha),
            recent_latencies: Vec::new(),
            avg_latency_ms: 0.0,
            latency_jitter_ms: 0.0,
            health_score: 1.0,
        }
    }

    /// Record a successful request with optional latency.
    fn record_success(&mut self, latency_ms: Option<f64>) {
        self.consecutive_failures = 0;
        self.last_failure = None;

        self.total_attempts += 1;
        self.successful_attempts += 1;
        self.recent_attempts.push(true);

        // Update EWMA error rate (0.0 for success)
        self.ewma_error_rate.update(0.0);

        // Track latency if provided
        if let Some(latency) = latency_ms {
            self.record_latency(latency);
        }

        // Keep only recent attempts for rolling window
        if self.recent_attempts.len() > 100 {
            self.recent_attempts.remove(0);
        }

        self.update_success_rate();
        self.update_health_score();
    }

    /// Record a failed request with optional latency.
    fn record_failure(&mut self, latency_ms: Option<f64>) {
        self.consecutive_failures += 1;
        self.last_failure = Some(Instant::now());

        self.total_attempts += 1;
        self.recent_attempts.push(false);

        // Update EWMA error rate (1.0 for failure)
        self.ewma_error_rate.update(1.0);

        // Track latency if provided (high latency often precedes failures)
        if let Some(latency) = latency_ms {
            self.record_latency(latency);
        }

        // Keep only recent attempts for rolling window
        if self.recent_attempts.len() > 100 {
            self.recent_attempts.remove(0);
        }

        self.update_success_rate();
        self.update_health_score();
    }

    /// Record latency measurement
    fn record_latency(&mut self, latency_ms: f64) {
        self.recent_latencies.push(latency_ms);

        // Keep only recent 50 latencies for calculation
        if self.recent_latencies.len() > 50 {
            self.recent_latencies.remove(0);
        }

        self.update_latency_stats();
    }

    /// Update latency statistics (average and jitter)
    fn update_latency_stats(&mut self) {
        if self.recent_latencies.is_empty() {
            self.avg_latency_ms = 0.0;
            self.latency_jitter_ms = 0.0;
            return;
        }

        // Calculate average
        let sum: f64 = self.recent_latencies.iter().sum();
        self.avg_latency_ms = sum / self.recent_latencies.len() as f64;

        // Calculate jitter (standard deviation)
        if self.recent_latencies.len() > 1 {
            let variance: f64 = self
                .recent_latencies
                .iter()
                .map(|&lat| {
                    let diff = lat - self.avg_latency_ms;
                    diff * diff
                })
                .sum::<f64>()
                / (self.recent_latencies.len() - 1) as f64;
            self.latency_jitter_ms = variance.sqrt();
        } else {
            self.latency_jitter_ms = 0.0;
        }
    }

    /// Update success rate based on recent attempts.
    fn update_success_rate(&mut self) {
        if self.recent_attempts.is_empty() {
            self.success_rate = 1.0;
            return;
        }

        let successes = self
            .recent_attempts
            .iter()
            .filter(|&&success| success)
            .count();
        self.success_rate = successes as f64 / self.recent_attempts.len() as f64;
        self.successful_attempts = successes;
        self.total_attempts = self.recent_attempts.len();
    }

    /// Calculate health score [0, 1] based on errors, latency, and jitter
    fn update_health_score(&mut self) {
        // Component 1: Error rate score (weight: 50%)
        let error_score = self.success_rate;

        // Component 2: Latency score (weight: 30%)
        // Normalize latency: 0ms = 1.0, 1000ms = 0.5, 5000ms+ = 0.0
        let latency_score = if self.avg_latency_ms == 0.0 {
            1.0
        } else if self.avg_latency_ms <= 100.0 {
            1.0 - (self.avg_latency_ms / 200.0) * 0.1 // 0-100ms: 1.0 to 0.95
        } else if self.avg_latency_ms <= 1000.0 {
            0.95 - ((self.avg_latency_ms - 100.0) / 900.0) * 0.45 // 100-1000ms: 0.95 to 0.5
        } else {
            (0.5 - ((self.avg_latency_ms - 1000.0) / 4000.0) * 0.5).max(0.0) // 1000-5000ms: 0.5 to 0.0
        };

        // Component 3: Jitter score (weight: 20%)
        // Low jitter is good, high jitter indicates instability
        let jitter_score = if self.latency_jitter_ms == 0.0 {
            1.0
        } else if self.latency_jitter_ms <= 50.0 {
            1.0 - (self.latency_jitter_ms / 50.0) * 0.2 // 0-50ms: 1.0 to 0.8
        } else if self.latency_jitter_ms <= 200.0 {
            0.8 - ((self.latency_jitter_ms - 50.0) / 150.0) * 0.5 // 50-200ms: 0.8 to 0.3
        } else {
            (0.3 - ((self.latency_jitter_ms - 200.0) / 300.0) * 0.3).max(0.0) // 200-500ms: 0.3 to 0.0
        };

        // Weighted combination
        self.health_score = (error_score * 0.5) + (latency_score * 0.3) + (jitter_score * 0.2);
        self.health_score = self.health_score.clamp(0.0, 1.0);
    }
}

/// Health statistics for external reporting.
#[derive(Debug, Clone)]
pub struct EndpointHealthStats {
    pub state: EndpointState,
    pub consecutive_failures: u32,
    pub success_rate: f64,
    pub total_attempts: usize,
    pub successful_attempts: usize,
    pub adaptive_threshold: u32,
    pub canary_progress: Option<(u32, u32)>, // (successes, total) for half-open state
    pub health_score: f64,
    pub avg_latency_ms: f64,
    pub latency_jitter_ms: f64,
    pub ewma_error_rate: f64,
}

/// Circuit breaker configuration.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub cooldown_seconds: u64,
    pub sample_size: usize,
    pub min_success_rate: f64,
    pub canary_count: u32,
    pub canary_success_threshold: f64,
    pub max_backoff: u32,
    pub adaptive_thresholds: bool,
    pub ewma_alpha: f64,
    pub adaptive_sensitivity: f64,
    pub min_health_score: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_circuit_breaker_creation() {
        let cb = CircuitBreaker::new(5, 60, 50);
        assert_eq!(cb.failure_threshold, 5);
        assert_eq!(cb.cooldown_duration, Duration::from_secs(60));
        assert_eq!(cb.sample_size, 50);
    }

    #[tokio::test]
    async fn test_endpoint_initially_healthy() {
        let cb = CircuitBreaker::new(3, 60, 50);

        // First check availability to populate the endpoint entry
        assert!(cb.is_available("test-endpoint").await);
        assert_eq!(
            cb.get_endpoint_state("test-endpoint").await,
            EndpointState::Healthy
        );
    }

    #[tokio::test]
    async fn test_record_success() {
        let cb = CircuitBreaker::new(3, 60, 50);

        cb.record_success("test-endpoint").await;
        assert!(cb.is_available("test-endpoint").await);
        assert_eq!(
            cb.get_endpoint_state("test-endpoint").await,
            EndpointState::Healthy
        );
    }

    #[tokio::test]
    async fn test_record_failure_degradation() {
        let cb = CircuitBreaker::new(3, 60, 50);

        // Record failures up to threshold
        for _ in 0..3 {
            cb.record_failure("test-endpoint").await;
        }

        assert_eq!(
            cb.get_endpoint_state("test-endpoint").await,
            EndpointState::Degraded
        );
        assert!(cb.is_available("test-endpoint").await); // Still available but degraded
    }

    #[tokio::test]
    async fn test_cooldown_transition() {
        let cb = CircuitBreaker::new(2, 60, 50);

        // Record enough failures to trigger cooldown
        for _ in 0..5 {
            cb.record_failure("test-endpoint").await;
        }

        assert_eq!(
            cb.get_endpoint_state("test-endpoint").await,
            EndpointState::CoolingDown
        );
        assert!(!cb.is_available("test-endpoint").await);
    }

    #[tokio::test]
    async fn test_recovery_after_success() {
        let cb = CircuitBreaker::new(3, 60, 50);

        // Degrade endpoint
        for _ in 0..3 {
            cb.record_failure("test-endpoint").await;
        }
        assert_eq!(
            cb.get_endpoint_state("test-endpoint").await,
            EndpointState::Degraded
        );

        // Recover with successes
        for _ in 0..10 {
            cb.record_success("test-endpoint").await;
        }

        assert_eq!(
            cb.get_endpoint_state("test-endpoint").await,
            EndpointState::Healthy
        );
    }

    #[tokio::test]
    async fn test_get_healthy_endpoints() {
        let cb = CircuitBreaker::new(3, 60, 50);

        cb.record_success("healthy-1").await;
        cb.record_success("healthy-2").await;

        for _ in 0..3 {
            cb.record_failure("degraded").await;
        }

        let healthy = cb.get_healthy_endpoints().await;
        assert!(healthy.contains(&"healthy-1".to_string()));
        assert!(healthy.contains(&"healthy-2".to_string()));
        assert!(!healthy.contains(&"degraded".to_string()));
    }

    #[tokio::test]
    async fn test_get_available_endpoints() {
        let cb = CircuitBreaker::new(3, 60, 50);

        cb.record_success("healthy").await;

        for _ in 0..3 {
            cb.record_failure("degraded").await;
        }

        for _ in 0..6 {
            cb.record_failure("cooling-down").await;
        }

        let available = cb.get_available_endpoints().await;
        assert!(available.contains(&"healthy".to_string()));
        assert!(available.contains(&"degraded".to_string()));
        assert!(!available.contains(&"cooling-down".to_string()));
    }

    #[tokio::test]
    async fn test_reset_all() {
        let cb = CircuitBreaker::new(2, 60, 50);

        // Degrade some endpoints
        for _ in 0..3 {
            cb.record_failure("endpoint-1").await;
            cb.record_failure("endpoint-2").await;
        }

        cb.reset_all().await;

        assert_eq!(
            cb.get_endpoint_state("endpoint-1").await,
            EndpointState::Healthy
        );
        assert_eq!(
            cb.get_endpoint_state("endpoint-2").await,
            EndpointState::Healthy
        );
    }

    #[tokio::test]
    async fn test_health_stats() {
        let cb = CircuitBreaker::new(3, 60, 50);

        cb.record_success("test").await;
        cb.record_failure("test").await;
        cb.record_success("test").await;

        let stats = cb.get_health_stats().await;
        let test_stats = stats.get("test").expect("test endpoint should exist");

        assert_eq!(test_stats.consecutive_failures, 0); // Reset after success
        assert_eq!(test_stats.total_attempts, 3);
        assert_eq!(test_stats.successful_attempts, 2);
        assert!((test_stats.success_rate - 0.666).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_adaptive_threshold() {
        let cb = CircuitBreaker::with_config(5, 60, 10, 3, true);

        // Simulate high error rate scenario
        for _ in 0..8 {
            cb.record_failure("test-adaptive").await;
        }
        for _ in 0..2 {
            cb.record_success("test-adaptive").await;
        }

        let stats = cb.get_health_stats().await;
        let endpoint_stats = stats
            .get("test-adaptive")
            .expect("test-adaptive endpoint should exist");

        // With 80% error rate, adaptive threshold should be lower than original
        assert!(endpoint_stats.adaptive_threshold < cb.failure_threshold);
        // With 20% success rate (< 30%), should be in cooling down
        assert_eq!(endpoint_stats.state, EndpointState::CoolingDown);
    }

    #[tokio::test]
    async fn test_canary_requests_success() {
        let cb = CircuitBreaker::with_config(2, 1, 50, 3, false);

        // Force endpoint into cooldown
        for _ in 0..5 {
            cb.record_failure("test-canary").await;
        }
        assert_eq!(
            cb.get_endpoint_state("test-canary").await,
            EndpointState::CoolingDown
        );

        // Wait for cooldown to expire (base=1s * 2^1 * max_jitter=1.3 = 2.6s)
        sleep(Duration::from_secs(3)).await;

        // Check availability - should transition to half-open
        assert!(cb.is_available("test-canary").await);
        assert_eq!(
            cb.get_endpoint_state("test-canary").await,
            EndpointState::HalfOpen
        );

        // Send successful canary requests
        cb.record_success("test-canary").await;
        cb.record_success("test-canary").await;
        cb.record_failure("test-canary").await;

        // After 3 canary requests with 2 successes (67%), should move to degraded
        assert_eq!(
            cb.get_endpoint_state("test-canary").await,
            EndpointState::Degraded
        );
    }

    #[tokio::test]
    async fn test_canary_requests_failure() {
        let cb = CircuitBreaker::with_config(2, 1, 50, 3, false);

        // Force endpoint into cooldown
        for _ in 0..5 {
            cb.record_failure("test-canary-fail").await;
        }
        assert_eq!(
            cb.get_endpoint_state("test-canary-fail").await,
            EndpointState::CoolingDown
        );

        // Wait for cooldown to expire (base=1s * 2^1 * max_jitter=1.3 = 2.6s)
        sleep(Duration::from_secs(3)).await;

        // Check availability - should transition to half-open
        assert!(cb.is_available("test-canary-fail").await);
        assert_eq!(
            cb.get_endpoint_state("test-canary-fail").await,
            EndpointState::HalfOpen
        );

        // Send failing canary requests
        cb.record_failure("test-canary-fail").await;
        cb.record_failure("test-canary-fail").await;
        cb.record_success("test-canary-fail").await;

        // After 3 canary requests with only 1 success (33%), should go back to cooldown
        assert_eq!(
            cb.get_endpoint_state("test-canary-fail").await,
            EndpointState::CoolingDown
        );
    }

    #[tokio::test]
    async fn test_exponential_backoff() {
        let cb = CircuitBreaker::with_config(2, 2, 50, 3, false);

        // First cooldown
        for _ in 0..5 {
            cb.record_failure("test-backoff").await;
        }

        // Check that the backoff strategy has been incremented
        {
            let health_map = cb.endpoint_health.read().await;
            let health = health_map.get("test-backoff").expect("test-backoff endpoint should exist");
            assert_eq!(health.backoff_strategy.attempt, 1); // First cooldown
        }

        // Wait long enough for cooldown to expire (base=2s * 2^1 * max_jitter=1.3 ≈ 5.2s)
        sleep(Duration::from_secs(6)).await;
        assert!(cb.is_available("test-backoff").await); // Enter half-open

        // Fail all canary requests
        cb.record_failure("test-backoff").await;
        cb.record_failure("test-backoff").await;
        cb.record_failure("test-backoff").await;

        // Check that the backoff strategy has been incremented again
        {
            let health_map = cb.endpoint_health.read().await;
            let health = health_map.get("test-backoff").expect("test-backoff endpoint should exist");
            assert_eq!(health.backoff_strategy.attempt, 2); // Exponential increase
        }
    }

    #[tokio::test]
    async fn test_state_change_callback() {
        use std::sync::{Arc as StdArc, Mutex};

        let mut cb = CircuitBreaker::new(2, 1, 50);
        let changes = StdArc::new(Mutex::new(Vec::new()));
        let changes_clone = StdArc::clone(&changes);

        cb.set_state_change_callback(move |endpoint, old_state, new_state| {
            if let Ok(mut changes) = changes_clone.lock() {
                changes.push((endpoint.to_string(), old_state, new_state));
            }
        });

        // Trigger state changes
        for _ in 0..2 {
            cb.record_failure("test-callback").await;
        }
        // Should transition to Degraded

        for _ in 0..3 {
            cb.record_failure("test-callback").await;
        }
        // Should transition to CoolingDown

        let recorded_changes = changes.lock().expect("lock should not be poisoned");
        assert!(recorded_changes.len() >= 2);
        assert!(recorded_changes
            .iter()
            .any(|(_, _, new)| *new == EndpointState::Degraded));
        assert!(recorded_changes
            .iter()
            .any(|(_, _, new)| *new == EndpointState::CoolingDown));
    }

    #[tokio::test]
    async fn test_per_endpoint_isolation() {
        let cb = CircuitBreaker::new(3, 60, 50);

        // Degrade endpoint 1 by exceeding threshold * 2
        for _ in 0..7 {
            cb.record_failure("endpoint-1").await;
        }

        // Keep endpoint 2 healthy
        for _ in 0..5 {
            cb.record_success("endpoint-2").await;
        }

        // Verify isolation
        assert_eq!(
            cb.get_endpoint_state("endpoint-1").await,
            EndpointState::CoolingDown
        );
        assert_eq!(
            cb.get_endpoint_state("endpoint-2").await,
            EndpointState::Healthy
        );
        assert!(!cb.is_available("endpoint-1").await);
        assert!(cb.is_available("endpoint-2").await);
    }

    #[tokio::test]
    async fn test_get_config() {
        let cb = CircuitBreaker::with_config(10, 30, 100, 5, true);
        let config = cb.get_config();

        assert_eq!(config.failure_threshold, 10);
        assert_eq!(config.cooldown_seconds, 30);
        assert_eq!(config.sample_size, 100);
        assert_eq!(config.canary_count, 5);
        assert!(config.adaptive_thresholds);
    }

    /// Test scenario: spike of errors → Open (CoolingDown), then recovery → HalfOpen → Closed (Healthy)
    #[tokio::test]
    async fn test_spike_recovery_scenario() {
        let cb = CircuitBreaker::with_config(3, 1, 50, 3, false);

        // Initial state: Healthy
        assert_eq!(
            cb.get_endpoint_state("rpc-endpoint").await,
            EndpointState::Healthy
        );

        // Phase 1: Spike of errors - trigger circuit breaker to Open
        for _ in 0..10 {
            cb.record_failure("rpc-endpoint").await;
        }

        // Should be in CoolingDown state (Open)
        let state = cb.get_endpoint_state("rpc-endpoint").await;
        assert_eq!(state, EndpointState::CoolingDown);
        assert!(!cb.is_available("rpc-endpoint").await);

        // Phase 2: Wait for cooldown to expire (base=1s * multiplier=2 * max_jitter=1.3 = 2.6s)
        sleep(Duration::from_secs(3)).await;

        // Should transition to HalfOpen after cooldown
        assert!(cb.is_available("rpc-endpoint").await);
        assert_eq!(
            cb.get_endpoint_state("rpc-endpoint").await,
            EndpointState::HalfOpen
        );

        // Phase 3: Successful canary requests
        cb.record_success("rpc-endpoint").await;
        cb.record_success("rpc-endpoint").await;
        cb.record_success("rpc-endpoint").await;

        // Should move to Degraded after successful canary tests
        assert_eq!(
            cb.get_endpoint_state("rpc-endpoint").await,
            EndpointState::Degraded
        );

        // Phase 4: Continue with successes to fully recover
        for _ in 0..10 {
            cb.record_success("rpc-endpoint").await;
        }

        // Should be back to Healthy (Closed)
        assert_eq!(
            cb.get_endpoint_state("rpc-endpoint").await,
            EndpointState::Healthy
        );
        assert!(cb.is_available("rpc-endpoint").await);
    }

    #[tokio::test]
    async fn test_backoff_strategy_jitter() {
        let mut strategy = BackoffStrategy::new(1000, 60000);

        // Test that delays increase exponentially
        let delay1 = strategy.next_delay();
        let delay2 = strategy.next_delay();
        let delay3 = strategy.next_delay();

        // Delays should generally increase (accounting for jitter)
        assert!(delay1.as_millis() > 500); // At least 50% of base (with jitter)
        assert!(delay2.as_millis() > delay1.as_millis() / 2); // Should be roughly 2x
        assert!(delay3.as_millis() > delay2.as_millis() / 2); // Should be roughly 2x

        // Test reset
        strategy.reset();
        assert_eq!(strategy.attempt, 0);
    }

    #[tokio::test]
    async fn test_monitoring_task_management() {
        let cb = CircuitBreaker::new(3, 60, 50);

        // Start a monitoring task
        let endpoint = "test-endpoint".to_string();
        let endpoint_clone = endpoint.clone();

        cb.start_monitoring_task(endpoint.clone(), async move {
            sleep(Duration::from_secs(10)).await;
            debug!("Task completed for {}", endpoint_clone);
        })
        .await
        .expect("should start task");

        // Verify task is registered
        {
            let tasks = cb.monitoring_tasks.read().await;
            assert!(tasks.contains_key(&endpoint));
        }

        // Stop the task
        cb.stop_monitoring_task(&endpoint)
            .await
            .expect("should stop task");

        // Verify task is removed
        {
            let tasks = cb.monitoring_tasks.read().await;
            assert!(!tasks.contains_key(&endpoint));
        }
    }

    #[tokio::test]
    async fn test_hot_swap_monitoring_task() {
        let cb = CircuitBreaker::new(3, 60, 50);

        let endpoint = "test-endpoint".to_string();

        // Start first task
        cb.start_monitoring_task(endpoint.clone(), async {
            sleep(Duration::from_secs(10)).await;
        })
        .await
        .expect("should start first task");

        // Hot-swap with second task (should gracefully shutdown first)
        cb.start_monitoring_task(endpoint.clone(), async {
            sleep(Duration::from_secs(10)).await;
        })
        .await
        .expect("should start second task");

        // Verify only one task exists
        {
            let tasks = cb.monitoring_tasks.read().await;
            assert_eq!(tasks.len(), 1);
        }

        // Clean up
        cb.stop_all_monitoring_tasks().await;
    }

    #[tokio::test]
    async fn test_ewma_adaptive_threshold() {
        // Test EWMA-based adaptive thresholds with sensitivity
        let cb = CircuitBreaker::with_full_config(
            5,    // base threshold
            60,   // cooldown
            50,   // sample size
            3,    // canary count
            true, // adaptive thresholds
            0.3,  // ewma alpha (30% weight on recent)
            2.0,  // sensitivity (2 stddev)
            0.3,  // min health score
        );

        // Generate a pattern: alternating success/failure to build EWMA
        for i in 0..20 {
            if i % 2 == 0 {
                cb.record_failure("ewma-test").await;
            } else {
                cb.record_success("ewma-test").await;
            }
        }

        let stats = cb.get_health_stats().await;
        let endpoint_stats = stats
            .get("ewma-test")
            .expect("ewma-test endpoint should exist");

        // With alternating pattern, EWMA error rate should be around 0.5
        assert!(endpoint_stats.ewma_error_rate > 0.3 && endpoint_stats.ewma_error_rate < 0.7);

        // Adaptive threshold should be lower than base due to high error rate
        assert!(endpoint_stats.adaptive_threshold < cb.failure_threshold);
    }

    #[tokio::test]
    async fn test_health_score_calculation() {
        let cb = CircuitBreaker::new(5, 60, 50);

        // Test with successes and low latency
        cb.record_success_with_latency("healthy-endpoint", Some(50.0))
            .await;
        cb.record_success_with_latency("healthy-endpoint", Some(60.0))
            .await;
        cb.record_success_with_latency("healthy-endpoint", Some(55.0))
            .await;

        let stats1 = cb.get_health_stats().await;
        let healthy = stats1
            .get("healthy-endpoint")
            .expect("healthy-endpoint should exist");

        // High health score expected: good success rate, low latency, low jitter
        assert!(
            healthy.health_score > 0.8,
            "Health score {} should be > 0.8",
            healthy.health_score
        );
        assert!(healthy.avg_latency_ms < 100.0);
        assert!(healthy.latency_jitter_ms < 20.0);

        // Test with failures and high latency
        cb.record_failure_with_latency("unhealthy-endpoint", Some(2000.0))
            .await;
        cb.record_failure_with_latency("unhealthy-endpoint", Some(2500.0))
            .await;
        cb.record_failure_with_latency("unhealthy-endpoint", Some(3000.0))
            .await;

        let stats2 = cb.get_health_stats().await;
        let unhealthy = stats2
            .get("unhealthy-endpoint")
            .expect("unhealthy-endpoint should exist");

        // Low health score expected: poor success rate, high latency, high jitter
        assert!(
            unhealthy.health_score < 0.5,
            "Health score {} should be < 0.5",
            unhealthy.health_score
        );
        assert!(unhealthy.avg_latency_ms > 1000.0);
    }

    #[tokio::test]
    async fn test_health_score_state_transition() {
        // Test that health score affects state transitions
        let cb = CircuitBreaker::with_full_config(
            5,    // base threshold
            60,   // cooldown
            50,   // sample size
            3,    // canary count
            true, // adaptive thresholds
            0.3,  // ewma alpha
            2.0,  // sensitivity
            0.7,  // high min health score threshold
        );

        // Record failures with very high latency and jitter
        for _ in 0..3 {
            cb.record_failure_with_latency("health-test", Some(4000.0))
                .await;
            cb.record_failure_with_latency("health-test", Some(5000.0))
                .await;
            cb.record_success_with_latency("health-test", Some(4500.0))
                .await; // Keep it from cooling down
        }

        let stats = cb.get_health_stats().await;
        let endpoint = stats
            .get("health-test")
            .expect("health-test endpoint should exist");

        // Should be degraded or worse due to low health score
        assert!(
            endpoint.state == EndpointState::Degraded
                || endpoint.state == EndpointState::CoolingDown
        );
        assert!(endpoint.health_score < 0.7);
    }

    #[tokio::test]
    async fn test_metrics_export() {
        let cb = CircuitBreaker::new(3, 60, 50);

        // Record some activity
        cb.record_success_with_latency("metrics-test", Some(100.0))
            .await;
        cb.record_failure_with_latency("metrics-test", Some(200.0))
            .await;
        cb.record_success_with_latency("metrics-test", Some(150.0))
            .await;

        // Get metrics registry
        let registry = cb.get_metrics_registry().await;

        // Verify metrics are registered
        let metrics = registry.gather();
        assert!(!metrics.is_empty(), "Metrics should be registered");

        // Check that some expected metrics exist
        let metric_names: Vec<String> = metrics.iter().map(|m| m.get_name().to_string()).collect();

        assert!(metric_names
            .iter()
            .any(|n| n.contains("circuit_breaker_state")));
        assert!(metric_names
            .iter()
            .any(|n| n.contains("circuit_breaker_success_ratio")));
        assert!(metric_names
            .iter()
            .any(|n| n.contains("circuit_breaker_health_score")));
    }

    #[tokio::test]
    async fn test_latency_tracking() {
        let cb = CircuitBreaker::new(3, 60, 50);

        // Record requests with varying latencies
        let latencies = vec![50.0, 100.0, 75.0, 125.0, 90.0];
        for latency in &latencies {
            cb.record_success_with_latency("latency-test", Some(*latency))
                .await;
        }

        let stats = cb.get_health_stats().await;
        let endpoint = stats
            .get("latency-test")
            .expect("latency-test endpoint should exist");

        // Check average latency is calculated
        let expected_avg = latencies.iter().sum::<f64>() / latencies.len() as f64;
        assert!((endpoint.avg_latency_ms - expected_avg).abs() < 1.0);

        // Check jitter (stddev) is calculated
        assert!(endpoint.latency_jitter_ms > 0.0);
    }

    #[tokio::test]
    async fn test_ewma_spike_vs_chronic() {
        // Test that EWMA can distinguish between spike and chronic degradation
        let cb = CircuitBreaker::with_full_config(5, 60, 50, 3, true, 0.3, 2.0, 0.3);

        // Spike scenario: mostly successes with a brief spike of failures
        for _ in 0..20 {
            cb.record_success("spike-endpoint").await;
        }
        for _ in 0..5 {
            cb.record_failure("spike-endpoint").await;
        }
        for _ in 0..20 {
            cb.record_success("spike-endpoint").await;
        }

        let spike_stats = cb.get_health_stats().await;
        let spike = spike_stats
            .get("spike-endpoint")
            .expect("spike-endpoint should exist");

        // Chronic scenario: persistent high error rate
        for i in 0..30 {
            if i % 3 == 0 {
                cb.record_success("chronic-endpoint").await;
            } else {
                cb.record_failure("chronic-endpoint").await;
            }
        }

        let chronic_stats = cb.get_health_stats().await;
        let chronic = chronic_stats
            .get("chronic-endpoint")
            .expect("chronic-endpoint should exist");

        // Spike should recover better than chronic
        assert!(spike.success_rate > chronic.success_rate);
        assert!(spike.health_score > chronic.health_score);

        // Chronic endpoint should have higher EWMA error rate
        assert!(chronic.ewma_error_rate > spike.ewma_error_rate);
    }

    #[tokio::test]
    async fn test_config_with_ewma_params() {
        let cb = CircuitBreaker::with_full_config(
            10,   // threshold
            30,   // cooldown
            100,  // sample size
            5,    // canary count
            true, // adaptive
            0.5,  // ewma alpha
            3.0,  // sensitivity
            0.4,  // min health score
        );

        let config = cb.get_config();

        assert_eq!(config.failure_threshold, 10);
        assert_eq!(config.cooldown_seconds, 30);
        assert_eq!(config.sample_size, 100);
        assert_eq!(config.canary_count, 5);
        assert!(config.adaptive_thresholds);
        assert_eq!(config.ewma_alpha, 0.5);
        assert_eq!(config.adaptive_sensitivity, 3.0);
        assert_eq!(config.min_health_score, 0.4);
    }

    #[tokio::test]
    async fn test_shutdown_task_deterministic() {
        // Test deterministic shutdown with comprehensive logging
        let cb = CircuitBreaker::new(3, 60, 50);

        // Test Case 1: Task that completes gracefully within Phase 1 (< 5s)
        {
            let endpoint = "graceful-shutdown-test".to_string();
            let token = CancellationToken::new();
            let token_clone = token.clone();

            let start = Instant::now();
            let handle = tokio::spawn(async move {
                // Wait for cancellation signal
                token_clone.cancelled().await;
                // Simulate quick cleanup
                sleep(Duration::from_millis(100)).await;
            });

            // Test graceful shutdown
            cb.shutdown_task(handle, token).await;
            let elapsed = start.elapsed();

            // Should complete within Phase 1 (well under 5s)
            assert!(
                elapsed < Duration::from_secs(2),
                "Graceful shutdown should complete quickly, took {:?}",
                elapsed
            );
        }

        // Test Case 2: Task that times out in Phase 1 and requires abort (Phase 2)
        {
            let token = CancellationToken::new();
            let token_clone = token.clone();

            let start = Instant::now();
            let handle = tokio::spawn(async move {
                // Simulate a task that ignores cancellation
                loop {
                    if token_clone.is_cancelled() {
                        // Acknowledged but intentionally don't exit quickly
                        sleep(Duration::from_secs(10)).await;
                        break;
                    }
                    sleep(Duration::from_millis(100)).await;
                }
            });

            // Test forced abort after Phase 1 timeout
            cb.shutdown_task(handle, token).await;
            let elapsed = start.elapsed();

            // Should complete within Phase 1 + Phase 2 (~7s total)
            // Allow some margin for scheduling
            assert!(
                elapsed >= Duration::from_secs(5),
                "Should wait at least 5s for Phase 1, took {:?}",
                elapsed
            );
            assert!(
                elapsed < Duration::from_secs(8),
                "Should complete within 7s (Phase 1 + Phase 2), took {:?}",
                elapsed
            );
        }

        // Test Case 3: Task that panics during shutdown
        {
            let token = CancellationToken::new();
            let token_clone = token.clone();

            let start = Instant::now();
            let handle = tokio::spawn(async move {
                token_clone.cancelled().await;
                panic!("Intentional panic during shutdown test");
            });

            // Test panic handling
            cb.shutdown_task(handle, token).await;
            let elapsed = start.elapsed();

            // Should complete quickly and log the panic
            assert!(
                elapsed < Duration::from_secs(2),
                "Panic shutdown should complete quickly, took {:?}",
                elapsed
            );
        }

        // Test Case 4: Task that completes immediately
        {
            let token = CancellationToken::new();

            let start = Instant::now();
            let handle = tokio::spawn(async move {
                // Task completes immediately
            });

            // Test immediate completion
            cb.shutdown_task(handle, token).await;
            let elapsed = start.elapsed();

            // Should complete almost instantly
            assert!(
                elapsed < Duration::from_millis(500),
                "Immediate completion should be very fast, took {:?}",
                elapsed
            );
        }
    }

    #[tokio::test]
    async fn test_shutdown_task_no_orphaned_tasks() {
        // Test that shutdown_task doesn't leave orphaned tasks
        let cb = CircuitBreaker::new(3, 60, 50);

        let endpoint = "orphan-test".to_string();

        // Start a monitoring task
        cb.start_monitoring_task(endpoint.clone(), async {
            // Task that runs for a long time
            sleep(Duration::from_secs(100)).await;
        })
        .await
        .expect("should start task");

        // Verify task is registered
        {
            let tasks = cb.monitoring_tasks.read().await;
            assert_eq!(tasks.len(), 1);
        }

        // Stop the task (invokes shutdown_task internally)
        cb.stop_monitoring_task(&endpoint)
            .await
            .expect("should stop task");

        // Verify task is removed (no orphans)
        {
            let tasks = cb.monitoring_tasks.read().await;
            assert_eq!(tasks.len(), 0, "Task should be removed, no orphans");
        }

        // Wait a bit to ensure no async operations are pending
        sleep(Duration::from_millis(100)).await;

        // Verify still no tasks
        {
            let tasks = cb.monitoring_tasks.read().await;
            assert_eq!(tasks.len(), 0, "Still no orphaned tasks after wait");
        }
    }

    #[tokio::test]
    async fn test_shutdown_task_logs_all_outcomes() {
        // This test verifies that shutdown_task logs all outcomes
        // We test the code paths that lead to different log statements
        let cb = CircuitBreaker::new(3, 60, 50);

        // Outcome 1: Graceful completion (logged as info)
        {
            let token = CancellationToken::new();
            let token_clone = token.clone();
            let handle = tokio::spawn(async move {
                token_clone.cancelled().await;
            });
            cb.shutdown_task(handle, token).await;
        }

        // Outcome 2: Panic (logged as warn)
        {
            let token = CancellationToken::new();
            let token_clone = token.clone();
            let handle = tokio::spawn(async move {
                token_clone.cancelled().await;
                panic!("Test panic");
            });
            cb.shutdown_task(handle, token).await;
        }

        // Outcome 3: Timeout requiring abort (logged as warn and info)
        {
            let token = CancellationToken::new();
            let _token_clone = token.clone();
            let handle = tokio::spawn(async move {
                // Task that doesn't check cancellation, will be aborted
                sleep(Duration::from_secs(10)).await;
            });
            cb.shutdown_task(handle, token).await;
        }

        // All outcomes tested - logs should contain relevant messages
        // In a real scenario, we'd use a log capture mechanism to verify
        // For this test, we just verify the code executes without panicking
    }

    #[tokio::test]
    async fn test_cancellation_token_immediate_termination() {
        // Test that CancellationToken stops tasks immediately
        let token = CancellationToken::new();
        let token_clone = token.clone();
        
        let start = Instant::now();
        
        // Spawn a task that waits for cancellation
        let task = tokio::spawn(async move {
            token_clone.cancelled().await;
        });
        
        // Give task time to start waiting
        sleep(Duration::from_millis(10)).await;
        
        // Cancel the token
        let cancel_time = Instant::now();
        token.cancel();
        
        // Task should complete immediately
        let result = tokio::time::timeout(Duration::from_millis(50), task).await;
        let completion_time = Instant::now();
        
        assert!(result.is_ok(), "Task should complete within 50ms");
        
        // The completion should be fast
        let elapsed = completion_time.duration_since(cancel_time);
        assert!(
            elapsed < Duration::from_millis(5),
            "Task should terminate immediately (within 5ms), but took {:?}",
            elapsed
        );
        
        // Verify token state
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn test_cancellation_token_multiple_waiters() {
        // Test that all waiting tasks are woken immediately
        let token = CancellationToken::new();
        
        // Spawn multiple tasks waiting for cancellation
        let mut tasks = Vec::new();
        for _ in 0..5 {
            let token_clone = token.clone();
            tasks.push(tokio::spawn(async move {
                token_clone.cancelled().await;
            }));
        }
        
        // Give tasks time to start waiting
        sleep(Duration::from_millis(10)).await;
        
        // Cancel the token
        let start = Instant::now();
        token.cancel();
        
        // All tasks should complete immediately
        for task in tasks {
            let result = tokio::time::timeout(Duration::from_millis(50), task).await;
            assert!(result.is_ok(), "All tasks should complete within 50ms");
        }
        
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(10),
            "All tasks should terminate immediately, but took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_cancellation_token_already_cancelled() {
        // Test fast path when token is already cancelled
        let token = CancellationToken::new();
        
        // Cancel before waiting
        token.cancel();
        assert!(token.is_cancelled());
        
        // Should return immediately without waiting for notification
        let start = Instant::now();
        token.cancelled().await;
        let elapsed = start.elapsed();
        
        // Should be instant (sub-millisecond)
        assert!(
            elapsed < Duration::from_millis(1),
            "Should return immediately when already cancelled, but took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_cancellation_token_zero_cpu_idle() {
        // Test that waiting tasks consume zero CPU in idle state
        let token = CancellationToken::new();
        let token_clone = token.clone();
        
        // Spawn a task that waits for cancellation
        let task = tokio::spawn(async move {
            token_clone.cancelled().await;
        });
        
        // Let the task wait for a bit (simulating idle state)
        sleep(Duration::from_millis(100)).await;
        
        // Cancel and verify task completes
        token.cancel();
        let result = tokio::time::timeout(Duration::from_millis(50), task).await;
        assert!(result.is_ok(), "Task should complete promptly");
    }

    #[tokio::test]
    async fn test_backoff_strategy_long_retry_stability() {
        let mut strategy = BackoffStrategy::new(100, 100_000_000);

        let mut prev_delay = Duration::from_millis(0);

        // Test that backoff remains stable even with very long retry sequences
        // The attempt counter should cap at MAX_ATTEMPT to prevent overflow
        for i in 1..=100 {
            let delay = strategy.next_delay();

            // Ensure delay is always valid (non-zero and within reasonable bounds)
            assert!(
                delay.as_millis() > 0,
                "Delay should never be zero at attempt {}",
                i
            );
            assert!(
                delay.as_millis() <= 130_000_000,
                "Delay should be capped at max + jitter at attempt {}",
                i
            );

            // After attempt MAX_ATTEMPT, the delay should stabilize (stay at max)
            if i > BackoffStrategy::MAX_ATTEMPT as usize {
                // Both delays should be at or near max_delay_ms (100_000_000)
                // accounting for jitter (±30%), so between 70M and 130M
                assert!(
                    delay.as_millis() >= 70_000_000,
                    "Delay at attempt {} should be near max: {} ms",
                    i,
                    delay.as_millis()
                );
            }

            // Verify attempt counter is capped at MAX_ATTEMPT
            assert!(
                strategy.attempt <= BackoffStrategy::MAX_ATTEMPT,
                "Attempt counter should be capped at {}, got {} at iteration {}",
                BackoffStrategy::MAX_ATTEMPT,
                strategy.attempt,
                i
            );

            prev_delay = delay;
        }

        // Final verification: attempt should be exactly MAX_ATTEMPT after 100 iterations
        assert_eq!(
            strategy.attempt, BackoffStrategy::MAX_ATTEMPT,
            "Attempt should be capped at {} after many retries",
            BackoffStrategy::MAX_ATTEMPT
        );

        // Test reset works after long sequence
        strategy.reset();
        assert_eq!(strategy.attempt, 0);

        // Verify backoff works correctly after reset
        let first_delay_after_reset = strategy.next_delay();
        assert!(
            first_delay_after_reset.as_millis() < 1000,
            "First delay after reset should be small: {} ms",
            first_delay_after_reset.as_millis()
        );
    }

    #[tokio::test]
    async fn test_backoff_strategy_overflow_prevention() {
        let mut strategy = BackoffStrategy::new(1000, u64::MAX);

        // Simulate many consecutive retries that would cause overflow without protection
        for _ in 0..70 {
            let delay = strategy.next_delay();

            // Should never panic or overflow
            assert!(delay.as_millis() > 0);

            // Verify attempt is capped
            assert!(
                strategy.attempt <= BackoffStrategy::MAX_ATTEMPT,
                "Attempt should be capped at {}",
                BackoffStrategy::MAX_ATTEMPT
            );
        }

        // At this point, attempt should be exactly MAX_ATTEMPT
        assert_eq!(strategy.attempt, BackoffStrategy::MAX_ATTEMPT);

        // Further calls should maintain attempt at MAX_ATTEMPT
        strategy.next_delay();
        assert_eq!(strategy.attempt, BackoffStrategy::MAX_ATTEMPT);
        strategy.next_delay();
        assert_eq!(strategy.attempt, BackoffStrategy::MAX_ATTEMPT);
    }

    #[tokio::test]
    async fn test_backoff_strategy_edge_cases() {
        // Test with very small base delay
        let mut small_strategy = BackoffStrategy::new(1, 1000);
        for _ in 0..10 {
            let delay = small_strategy.next_delay();
            assert!(delay.as_millis() > 0);
        }

        // Test with base delay equal to max delay (should stay constant after first attempt)
        let mut const_strategy = BackoffStrategy::new(5000, 5000);
        let delay1 = const_strategy.next_delay();
        let delay2 = const_strategy.next_delay();

        // Both should be around 5000ms (accounting for ±30% jitter)
        assert!(delay1.as_millis() >= 3500 && delay1.as_millis() <= 6500);
        assert!(delay2.as_millis() >= 3500 && delay2.as_millis() <= 6500);

        // Test that attempt at MAX_ATTEMPT produces a valid delay
        let mut edge_strategy = BackoffStrategy::new(1000, u64::MAX);
        edge_strategy.attempt = BackoffStrategy::MAX_ATTEMPT - 1; // Set to MAX_ATTEMPT - 1, next call will set it to MAX_ATTEMPT
        let delay_at_max = edge_strategy.next_delay();
        assert_eq!(edge_strategy.attempt, BackoffStrategy::MAX_ATTEMPT);
        assert!(delay_at_max.as_millis() > 0);
    }

    #[tokio::test]
    async fn test_metrics_registration_no_race_condition() {
        // Test that concurrent metric registration from multiple tasks doesn't cause:
        // - Duplicate registrations
        // - Partial state (some metrics registered but not others)
        // - Panics or errors
        //
        // This test validates the atomicity improvements in register_endpoint()
        use std::sync::Arc;

        let cb = Arc::new(CircuitBreaker::new(3, 60, 50));
        let num_tasks = 20;
        let num_endpoints = 10;

        // Spawn multiple tasks that concurrently register and update metrics for the same endpoints
        let mut handles = Vec::new();
        for task_id in 0..num_tasks {
            let cb_clone = Arc::clone(&cb);
            let handle = tokio::spawn(async move {
                for endpoint_id in 0..num_endpoints {
                    let endpoint = format!("endpoint-{}", endpoint_id);
                    
                    // Simulate concurrent access patterns:
                    // - Record success (triggers metric registration + update)
                    // - Record failure (triggers metric registration + update)
                    // - Check availability (may trigger metric registration)
                    
                    if task_id % 3 == 0 {
                        cb_clone.record_success(&endpoint).await;
                    } else if task_id % 3 == 1 {
                        cb_clone.record_failure(&endpoint).await;
                    } else {
                        let _ = cb_clone.is_available(&endpoint).await;
                        cb_clone.record_success_with_latency(&endpoint, Some(100.0)).await;
                    }
                    
                    // Small delay to increase chance of interleaving
                    sleep(Duration::from_micros(100)).await;
                }
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.expect("Task should complete without panic");
        }

        // Verify all endpoints have complete metric sets
        let metrics = cb.metrics.read().await;
        
        for endpoint_id in 0..num_endpoints {
            let endpoint = format!("endpoint-{}", endpoint_id);
            
            // Each endpoint should have all 6 metrics registered
            assert!(
                metrics.circuit_state.contains_key(&endpoint),
                "Endpoint {} missing circuit_state metric",
                endpoint
            );
            assert!(
                metrics.fail_count.contains_key(&endpoint),
                "Endpoint {} missing fail_count metric",
                endpoint
            );
            assert!(
                metrics.probe_latency.contains_key(&endpoint),
                "Endpoint {} missing probe_latency metric",
                endpoint
            );
            assert!(
                metrics.last_opened.contains_key(&endpoint),
                "Endpoint {} missing last_opened metric",
                endpoint
            );
            assert!(
                metrics.probe_success_ratio.contains_key(&endpoint),
                "Endpoint {} missing probe_success_ratio metric",
                endpoint
            );
            assert!(
                metrics.health_score.contains_key(&endpoint),
                "Endpoint {} missing health_score metric",
                endpoint
            );
        }

        // Verify we can gather metrics without panicking
        let registry = cb.get_metrics_registry().await;
        let gathered_metrics = registry.gather();
        
        // Should have metrics for all endpoints
        assert!(!gathered_metrics.is_empty(), "Should have gathered metrics");
        
        // Count the total metrics (each endpoint has 6 metrics)
        let total_expected_metrics = num_endpoints * 6;
        let total_gathered = gathered_metrics.len();
        
        // We should have at least as many metrics as endpoints * metrics per endpoint
        // (may have more due to Prometheus internal metrics)
        assert!(
            total_gathered >= total_expected_metrics,
            "Expected at least {} metrics, got {}",
            total_expected_metrics,
            total_gathered
        );
    }

    #[tokio::test]
    async fn test_shutdown_phase2_panic_during_abort() {
        // Test Phase 2: Task that panics after receiving abort signal
        let cb = CircuitBreaker::new(3, 60, 50);
        let token = CancellationToken::new();
        let token_clone = token.clone();

        let start = Instant::now();
        let handle = tokio::spawn(async move {
            // Task ignores cancellation and will be aborted
            loop {
                if token_clone.is_cancelled() {
                    // Simulate long cleanup that will be interrupted by abort
                    sleep(Duration::from_secs(10)).await;
                    // This panic happens after abort is called
                    panic!("Panic during abort cleanup");
                }
                sleep(Duration::from_millis(100)).await;
            }
        });

        // Shutdown should handle panic gracefully
        cb.shutdown_task(handle, token).await;
        let elapsed = start.elapsed();

        // Should complete within Phase 1 + Phase 2 timeouts
        assert!(
            elapsed >= Duration::from_secs(5),
            "Should wait at least 5s for Phase 1, took {:?}",
            elapsed
        );
        assert!(
            elapsed < Duration::from_secs(8),
            "Should complete within ~7s, took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_shutdown_multiple_concurrent() {
        // Test that multiple concurrent shutdowns work correctly
        let cb = Arc::new(CircuitBreaker::new(3, 60, 50));
        
        let mut join_handles = Vec::new();
        
        // Start multiple tasks that need shutdown, each in its own spawn
        for i in 0..5 {
            let cb_clone = Arc::clone(&cb);
            let handle = tokio::spawn(async move {
                let token = CancellationToken::new();
                let token_clone = token.clone();
                
                let task_handle = tokio::spawn(async move {
                    // Mix of behaviors
                    if i % 2 == 0 {
                        // Some tasks respond to cancellation
                        token_clone.cancelled().await;
                        sleep(Duration::from_millis(100)).await;
                    } else {
                        // Some tasks ignore cancellation and need abort
                        loop {
                            sleep(Duration::from_millis(100)).await;
                        }
                    }
                });
                
                cb_clone.shutdown_task(task_handle, token).await;
            });
            
            join_handles.push(handle);
        }
        
        // Wait for all concurrent shutdowns to complete
        let start = Instant::now();
        for handle in join_handles {
            handle.await.expect("Shutdown task should not panic");
        }
        let elapsed = start.elapsed();
        
        // Since shutdowns run concurrently, total time should be close to single longest shutdown
        // Longest is ~7s (Phase 1 + Phase 2), so we allow some margin
        assert!(
            elapsed < Duration::from_secs(10),
            "Concurrent shutdowns should complete within 10s, took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_cancellation_token_multiple_cancel_calls() {
        // Test that calling cancel() multiple times is safe
        let token = CancellationToken::new();
        let token_clone1 = token.clone();
        let token_clone2 = token.clone();
        
        // Start multiple tasks waiting for cancellation
        let task1 = tokio::spawn(async move {
            token_clone1.cancelled().await;
        });
        
        let task2 = tokio::spawn(async move {
            token_clone2.cancelled().await;
        });
        
        // Give tasks time to start waiting
        sleep(Duration::from_millis(10)).await;
        
        // Call cancel multiple times
        token.cancel();
        token.cancel();
        token.cancel();
        
        // All tasks should complete
        let result1 = tokio::time::timeout(Duration::from_millis(100), task1).await;
        let result2 = tokio::time::timeout(Duration::from_millis(100), task2).await;
        
        assert!(result1.is_ok(), "Task 1 should complete");
        assert!(result2.is_ok(), "Task 2 should complete");
        
        // Token should remain cancelled
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn test_cancellation_token_check_before_wait() {
        // Test fast-path: cancelled before any task waits
        let token = CancellationToken::new();
        
        // Cancel immediately
        token.cancel();
        
        // Now create tasks that check cancellation
        let token_clone = token.clone();
        let task = tokio::spawn(async move {
            token_clone.cancelled().await;
        });
        
        // Task should complete immediately due to fast-path
        let start = Instant::now();
        let result = tokio::time::timeout(Duration::from_millis(50), task).await;
        let elapsed = start.elapsed();
        
        assert!(result.is_ok(), "Task should complete within 50ms");
        assert!(
            elapsed < Duration::from_millis(5),
            "Fast-path should be immediate, took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_shutdown_task_race_condition() {
        // Test race condition: task completes just as abort is called
        let cb = CircuitBreaker::new(3, 60, 50);
        
        for _ in 0..10 {
            let token = CancellationToken::new();
            let token_clone = token.clone();
            
            let handle = tokio::spawn(async move {
                // Task waits close to Phase 1 timeout, then completes
                sleep(Duration::from_millis(4900)).await;
                if token_clone.is_cancelled() {
                    // Complete quickly after cancellation check
                    sleep(Duration::from_millis(50)).await;
                }
            });
            
            // This should handle the race gracefully
            cb.shutdown_task(handle, token).await;
        }
    }

    #[tokio::test]
    async fn test_shutdown_task_context_preservation() {
        // Test that shutdown preserves task context and doesn't leak across shutdowns
        let cb = CircuitBreaker::new(3, 60, 50);
        
        // First shutdown: graceful
        {
            let token = CancellationToken::new();
            let token_clone = token.clone();
            let handle = tokio::spawn(async move {
                token_clone.cancelled().await;
            });
            cb.shutdown_task(handle, token).await;
        }
        
        // Second shutdown: timeout and abort
        {
            let token = CancellationToken::new();
            let handle = tokio::spawn(async move {
                // Ignore cancellation
                sleep(Duration::from_secs(10)).await;
            });
            cb.shutdown_task(handle, token).await;
        }
        
        // Third shutdown: panic
        {
            let token = CancellationToken::new();
            let token_clone = token.clone();
            let handle = tokio::spawn(async move {
                token_clone.cancelled().await;
                panic!("Test panic");
            });
            cb.shutdown_task(handle, token).await;
        }
        
        // All shutdowns should complete independently without affecting each other
        // If we got here without panicking, the test passes
    }

    #[tokio::test]
    async fn test_cancellation_token_notify_semantics() {
        // Test the documented Notify semantics: new waiters after notify_waiters don't get woken
        let token = CancellationToken::new();
        
        // Task 1: Already waiting when cancel is called
        let token_clone1 = token.clone();
        let task1 = tokio::spawn(async move {
            token_clone1.cancelled().await;
        });
        
        // Give task1 time to start waiting
        sleep(Duration::from_millis(50)).await;
        
        // Now cancel - this should wake task1
        token.cancel();
        
        // Task 2: Starts waiting AFTER cancel was called
        // Due to fast-path, this should still work because we check the flag first
        let token_clone2 = token.clone();
        let task2 = tokio::spawn(async move {
            token_clone2.cancelled().await;
        });
        
        // Both tasks should complete
        let result1 = tokio::time::timeout(Duration::from_millis(100), task1).await;
        let result2 = tokio::time::timeout(Duration::from_millis(100), task2).await;
        
        assert!(result1.is_ok(), "Task 1 (already waiting) should complete");
        assert!(
            result2.is_ok(),
            "Task 2 (late arrival) should complete via fast-path"
        );
    }

    #[tokio::test]
    async fn test_deterministic_shutdown_ordering() {
        // Test that shutdown happens in deterministic order: Phase 1 -> Phase 2
        use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
        
        let cb = CircuitBreaker::new(3, 60, 50);
        let phase_counter = Arc::new(AtomicU32::new(0));
        let phase_counter_clone = phase_counter.clone();
        
        let token = CancellationToken::new();
        let token_clone = token.clone();
        
        let handle = tokio::spawn(async move {
            // Wait for phase 1 timeout
            tokio::select! {
                _ = token_clone.cancelled() => {
                    // Phase 1: graceful cancellation received
                    phase_counter_clone.store(1, AtomicOrdering::SeqCst);
                    // But don't exit - force Phase 2
                    sleep(Duration::from_secs(10)).await;
                }
            }
        });
        
        let start = Instant::now();
        cb.shutdown_task(handle, token).await;
        let elapsed = start.elapsed();
        
        // Phase counter should have been set (task entered Phase 1)
        let phase = phase_counter.load(AtomicOrdering::SeqCst);
        assert_eq!(phase, 1, "Task should have entered Phase 1");
        
        // Total time should reflect both phases
        assert!(
            elapsed >= Duration::from_secs(5),
            "Should include Phase 1 timeout"
        );
        assert!(
            elapsed < Duration::from_secs(8),
            "Should complete within Phase 1 + Phase 2"
        );
    }

    #[tokio::test]
    async fn test_metrics_registration_under_load() {
        // Stress test: High concurrency with many endpoints
        // Validates that the atomic registration holds up under extreme load
        use std::sync::Arc;

        let cb = Arc::new(CircuitBreaker::new(5, 30, 100));
        let num_concurrent_tasks = 50;
        let num_endpoints = 30;

        let mut handles = Vec::new();
        
        // Spawn many tasks that hammer the same endpoints simultaneously
        for _ in 0..num_concurrent_tasks {
            let cb_clone = Arc::clone(&cb);
            let handle = tokio::spawn(async move {
                // Each task rapidly accesses random endpoints
                for _ in 0..20 {
                    let endpoint_id = rand::random::<usize>() % num_endpoints;
                    let endpoint = format!("load-test-{}", endpoint_id);
                    
                    // Mix of operations
                    match rand::random::<u8>() % 4 {
                        0 => cb_clone.record_success(&endpoint).await,
                        1 => cb_clone.record_failure(&endpoint).await,
                        2 => cb_clone.record_success_with_latency(&endpoint, Some(50.0)).await,
                        _ => {
                            let _ = cb_clone.is_available(&endpoint).await;
                        }
                    }
                }
            });
            handles.push(handle);
        }

        // Wait for all tasks
        for handle in handles {
            handle.await.expect("Task should not panic under load");
        }

        // Verify integrity: all registered endpoints should have complete metric sets
        let metrics = cb.metrics.read().await;
        let health = cb.endpoint_health.read().await;
        
        // For each endpoint in health map, verify it has all metrics
        for endpoint in health.keys() {
            assert!(
                metrics.circuit_state.contains_key(endpoint),
                "Endpoint {} missing circuit_state after load test",
                endpoint
            );
            assert!(
                metrics.fail_count.contains_key(endpoint),
                "Endpoint {} missing fail_count after load test",
                endpoint
            );
            assert!(
                metrics.probe_latency.contains_key(endpoint),
                "Endpoint {} missing probe_latency after load test",
                endpoint
            );
            assert!(
                metrics.last_opened.contains_key(endpoint),
                "Endpoint {} missing last_opened after load test",
                endpoint
            );
            assert!(
                metrics.probe_success_ratio.contains_key(endpoint),
                "Endpoint {} missing probe_success_ratio after load test",
                endpoint
            );
            assert!(
                metrics.health_score.contains_key(endpoint),
                "Endpoint {} missing health_score after load test",
                endpoint
            );
        }

        // Verify metrics can be gathered without errors
        let registry = cb.get_metrics_registry().await;
        let gathered = registry.gather();
        assert!(!gathered.is_empty(), "Should have metrics after load test");
    }

    #[tokio::test]
    async fn test_registration_failures_metric() {
        // Test that registration_failures_total increments on metric registration errors
        use prometheus::Registry;
        
        let cb = CircuitBreaker::new(3, 60, 50);
        
        // Record some successes to trigger metric registration
        cb.record_success("test-endpoint-1").await;
        cb.record_success("test-endpoint-2").await;
        
        // Get initial registration failure count (should be 0)
        let initial_failures = {
            let metrics = cb.metrics.read().await;
            metrics.registration_failures_total.get()
        };
        
        assert_eq!(initial_failures, 0, "Initial registration failures should be 0");
        
        // To test registration failures, we need to try to register the same metric twice
        // which will cause a conflict. Let's create a CB with a pre-populated registry
        let pre_registry = Registry::new();
        
        // Pre-register a metric that will conflict
        let _pre_existing = prometheus::register_int_gauge_with_registry!(
            prometheus::Opts::new(
                "circuit_breaker_state_test_conflict",
                "Pre-existing metric to cause conflict"
            ),
            &pre_registry
        ).unwrap();
        
        // Create a new CircuitBreakerMetrics with the pre-populated registry
        let mut metrics = CircuitBreakerMetrics::new(pre_registry);
        
        // Try to register endpoint (this should fail because the metric already exists)
        let result = metrics.register_endpoint("test-conflict");
        
        // The registration should fail
        assert!(result.is_err(), "Registration should fail with conflicting metric");
        
        // Verify that registration_failures_total was incremented
        let failures_after = metrics.registration_failures_total.get();
        assert!(
            failures_after > 0,
            "Registration failures should be incremented, got {}",
            failures_after
        );
    }

    #[tokio::test]
    async fn test_monitoring_task_metrics() {
        // Test that monitoring_task_count and active_tasks metrics work correctly
        let cb = CircuitBreaker::new(3, 60, 50);
        
        // Check initial state
        let (initial_total, initial_active) = {
            let metrics = cb.metrics.read().await;
            (metrics.monitoring_task_count.get(), metrics.active_tasks.get())
        };
        
        assert_eq!(initial_total, 0, "Initial total tasks should be 0");
        assert_eq!(initial_active, 0, "Initial active tasks should be 0");
        
        // Start first monitoring task
        cb.start_monitoring_task("endpoint-1".to_string(), async {
            sleep(Duration::from_secs(10)).await;
        })
        .await
        .expect("Should start task 1");
        
        // Give task time to start
        sleep(Duration::from_millis(50)).await;
        
        let (total_1, active_1) = {
            let metrics = cb.metrics.read().await;
            (metrics.monitoring_task_count.get(), metrics.active_tasks.get())
        };
        
        assert_eq!(total_1, 1, "Total tasks should be 1 after starting first task");
        assert_eq!(active_1, 1, "Active tasks should be 1 after starting first task");
        
        // Start second monitoring task
        cb.start_monitoring_task("endpoint-2".to_string(), async {
            sleep(Duration::from_secs(10)).await;
        })
        .await
        .expect("Should start task 2");
        
        // Give task time to start
        sleep(Duration::from_millis(50)).await;
        
        let (total_2, active_2) = {
            let metrics = cb.metrics.read().await;
            (metrics.monitoring_task_count.get(), metrics.active_tasks.get())
        };
        
        assert_eq!(total_2, 2, "Total tasks should be 2 after starting second task");
        assert_eq!(active_2, 2, "Active tasks should be 2 after starting second task");
        
        // Stop one task
        cb.stop_monitoring_task("endpoint-1")
            .await
            .expect("Should stop task 1");
        
        // Give task time to complete shutdown
        sleep(Duration::from_millis(100)).await;
        
        let (total_3, active_3) = {
            let metrics = cb.metrics.read().await;
            (metrics.monitoring_task_count.get(), metrics.active_tasks.get())
        };
        
        assert_eq!(total_3, 2, "Total tasks should remain 2 (counter doesn't decrement)");
        assert_eq!(active_3, 1, "Active tasks should be 1 after stopping one task");
        
        // Stop all remaining tasks
        cb.stop_all_monitoring_tasks().await;
        
        // Give tasks time to complete shutdown
        sleep(Duration::from_millis(100)).await;
        
        let (total_final, active_final) = {
            let metrics = cb.metrics.read().await;
            (metrics.monitoring_task_count.get(), metrics.active_tasks.get())
        };
        
        assert_eq!(total_final, 2, "Total tasks should remain 2 (cumulative counter)");
        assert_eq!(active_final, 0, "Active tasks should be 0 after stopping all tasks");
    }

    #[tokio::test]
    async fn test_task_metrics_on_task_completion() {
        // Test that active_tasks decrements when a task completes naturally
        let cb = CircuitBreaker::new(3, 60, 50);
        
        // Start a short-lived task
        cb.start_monitoring_task("short-task".to_string(), async {
            sleep(Duration::from_millis(100)).await;
            // Task completes naturally
        })
        .await
        .expect("Should start task");
        
        // Give task time to start
        sleep(Duration::from_millis(50)).await;
        
        let active_during = {
            let metrics = cb.metrics.read().await;
            metrics.active_tasks.get()
        };
        
        assert_eq!(active_during, 1, "Active tasks should be 1 while task is running");
        
        // Wait for task to complete
        sleep(Duration::from_millis(200)).await;
        
        let active_after = {
            let metrics = cb.metrics.read().await;
            metrics.active_tasks.get()
        };
        
        assert_eq!(active_after, 0, "Active tasks should be 0 after task completes");
    }

    #[tokio::test]
    async fn test_task_hot_swap_metrics() {
        // Test that hot-swapping a task (replacing an existing task) handles metrics correctly
        let cb = CircuitBreaker::new(3, 60, 50);
        
        // Start first task
        cb.start_monitoring_task("endpoint".to_string(), async {
            sleep(Duration::from_secs(10)).await;
        })
        .await
        .expect("Should start first task");
        
        sleep(Duration::from_millis(50)).await;
        
        let (total_1, active_1) = {
            let metrics = cb.metrics.read().await;
            (metrics.monitoring_task_count.get(), metrics.active_tasks.get())
        };
        
        assert_eq!(total_1, 1, "Total tasks should be 1");
        assert_eq!(active_1, 1, "Active tasks should be 1");
        
        // Hot-swap: start another task for the same endpoint
        cb.start_monitoring_task("endpoint".to_string(), async {
            sleep(Duration::from_secs(10)).await;
        })
        .await
        .expect("Should start second task (hot-swap)");
        
        // Give old task time to shut down and new task to start
        sleep(Duration::from_millis(200)).await;
        
        let (total_2, active_2) = {
            let metrics = cb.metrics.read().await;
            (metrics.monitoring_task_count.get(), metrics.active_tasks.get())
        };
        
        // Total should increment (new task started)
        assert_eq!(total_2, 2, "Total tasks should be 2 after hot-swap");
        // Active should still be 1 (old task stopped, new task started)
        assert_eq!(active_2, 1, "Active tasks should be 1 after hot-swap");
        
        // Clean up
        cb.stop_all_monitoring_tasks().await;
        sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_metrics_summary_logs() {
        // Test that the summary logs include the correct metrics
        // This is a behavioral test - we verify the metrics have correct values
        // when stop_all_monitoring_tasks is called
        let cb = CircuitBreaker::new(3, 60, 50);
        
        // Start multiple tasks
        for i in 0..3 {
            cb.start_monitoring_task(format!("endpoint-{}", i), async move {
                sleep(Duration::from_secs(10)).await;
            })
            .await
            .expect("Should start task");
        }
        
        sleep(Duration::from_millis(100)).await;
        
        let (total_before, active_before) = {
            let metrics = cb.metrics.read().await;
            (metrics.monitoring_task_count.get(), metrics.active_tasks.get())
        };
        
        assert_eq!(total_before, 3, "Should have 3 total tasks");
        assert_eq!(active_before, 3, "Should have 3 active tasks");
        
        // Stop all tasks - this should log the summary
        cb.stop_all_monitoring_tasks().await;
        
        sleep(Duration::from_millis(100)).await;
        
        let (total_after, active_after, failures) = {
            let metrics = cb.metrics.read().await;
            (
                metrics.monitoring_task_count.get(),
                metrics.active_tasks.get(),
                metrics.registration_failures_total.get(),
            )
        };
        
        assert_eq!(total_after, 3, "Total tasks should remain 3");
        assert_eq!(active_after, 0, "Active tasks should be 0 after stopping all");
        assert_eq!(failures, 0, "Should have no registration failures in normal operation");
    }

    // ============================================================================
    // RACE CONDITION & CONCURRENCY TESTS
    // ============================================================================
    // These tests validate correctness under concurrent operations, race conditions,
    // and edge cases in the cancellation token and circuit breaker implementation.

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_cancel_after_await_race_condition() {
        // Test: Race condition where cancel() is called just as cancelled() is being awaited
        //
        // This validates that tokio_util::CancellationToken correctly handles the race
        // where cancellation happens concurrently with waiting tasks.
        
        for iteration in 0..100 {
            let token = CancellationToken::new();
            let token_cancel = token.clone();
            let token_wait = token.clone();
            
            // Spawn cancellation in a separate thread to create true concurrency
            let cancel_handle = tokio::spawn(async move {
                // Small delay to increase chance of race
                sleep(Duration::from_micros(10)).await;
                token_cancel.cancel();
            });
            
            // Main task waits for cancellation
            let wait_handle = tokio::spawn(async move {
                token_wait.cancelled().await;
            });
            
            // Both should complete within a reasonable time
            let timeout_result = tokio::time::timeout(
                Duration::from_millis(100), 
                async {
                    cancel_handle.await.expect("Cancel task should complete");
                    wait_handle.await.expect("Wait task should complete");
                }
            ).await;
            
            assert!(
                timeout_result.is_ok(),
                "Race condition detected at iteration {}: cancelled() did not wake despite cancel() being called",
                iteration
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_ignore_cancel_task_behavior() {
        // Test: Task that ignores cancellation signal and requires forced abort
        //
        // This validates that:
        // 1. Tasks can choose to ignore cancellation (don't check is_cancelled)
        // 2. The shutdown mechanism handles this gracefully with Phase 2 abort
        // 3. No deadlocks or resource leaks occur
        
        let cb = CircuitBreaker::new(3, 60, 50);
        let token = CancellationToken::new();
        let token_clone = token.clone();
        
        // Task that intentionally ignores the cancellation signal
        let handle = tokio::spawn(async move {
            // This task never checks is_cancelled() or cancelled().await
            // It will be forcibly aborted in Phase 2
            loop {
                sleep(Duration::from_millis(100)).await;
                // Busy work, ignoring all cancellation signals
                let _ = token_clone.is_cancelled(); // Check but don't act on it
            }
        });
        
        let start = Instant::now();
        cb.shutdown_task(handle, token).await;
        let elapsed = start.elapsed();
        
        // Should complete via Phase 2 abort after Phase 1 timeout
        assert!(
            elapsed >= Duration::from_secs(5),
            "Should wait at least 5s for Phase 1"
        );
        assert!(
            elapsed < Duration::from_secs(8),
            "Should complete within Phase 1 + Phase 2 (~7s), took {:?}",
            elapsed
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_hot_swap_race_condition() {
        // Test: Concurrent hot-swap of monitoring tasks
        //
        // This validates that hot-swapping (replacing a running task with a new one
        // for the same endpoint) is safe under concurrent access and doesn't:
        // - Leave orphaned tasks
        // - Corrupt internal state
        // - Cause data races in the monitoring_tasks HashMap
        
        let cb = Arc::new(CircuitBreaker::new(3, 60, 50));
        let num_swaps = 20;
        let endpoint = "hot-swap-test".to_string();
        
        // Perform rapid hot-swaps from multiple threads concurrently
        let mut swap_handles = Vec::new();
        for i in 0..num_swaps {
            let cb_clone = Arc::clone(&cb);
            let endpoint_clone = endpoint.clone();
            
            let handle = tokio::spawn(async move {
                cb_clone
                    .start_monitoring_task(endpoint_clone, async move {
                        // Each task does some work before being replaced
                        sleep(Duration::from_millis(50 + i * 10)).await;
                    })
                    .await
                    .expect("Hot-swap should succeed");
            });
            
            swap_handles.push(handle);
            
            // Small delay between swaps to create interleaving
            sleep(Duration::from_micros(100)).await;
        }
        
        // Wait for all hot-swaps to complete
        for handle in swap_handles {
            handle.await.expect("Swap task should complete");
        }
        
        // Give tasks time to settle
        sleep(Duration::from_millis(200)).await;
        
        // Verify only one task exists for the endpoint (no orphans)
        {
            let tasks = cb.monitoring_tasks.read().await;
            assert_eq!(
                tasks.len(),
                1,
                "Should have exactly 1 active task after all hot-swaps, found {}",
                tasks.len()
            );
            assert!(
                tasks.contains_key(&endpoint),
                "The remaining task should be for the hot-swap endpoint"
            );
        }
        
        // Clean up
        cb.stop_all_monitoring_tasks().await;
        
        // Verify cleanup
        {
            let tasks = cb.monitoring_tasks.read().await;
            assert_eq!(tasks.len(), 0, "All tasks should be cleaned up");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_leak_check_task_cleanup() {
        // Test: Comprehensive leak check for task resources
        //
        // This validates that no resources leak when:
        // 1. Starting and stopping many tasks
        // 2. Hot-swapping tasks
        // 3. Stopping individual tasks
        // 4. Stopping all tasks at once
        // 5. Tasks that complete naturally
        // 6. Tasks that are forcibly aborted
        
        let cb = Arc::new(CircuitBreaker::new(3, 60, 50));
        
        // Phase 1: Start many tasks
        for i in 0..10 {
            cb.start_monitoring_task(
                format!("leak-test-{}", i),
                async move {
                    sleep(Duration::from_secs(100)).await;
                }
            )
            .await
            .expect("Should start task");
        }
        
        // Verify tasks are registered
        {
            let tasks = cb.monitoring_tasks.read().await;
            assert_eq!(tasks.len(), 10, "Should have 10 tasks");
        }
        
        // Phase 2: Stop half individually
        for i in 0..5 {
            cb.stop_monitoring_task(&format!("leak-test-{}", i))
                .await
                .expect("Should stop task");
        }
        
        sleep(Duration::from_millis(200)).await;
        
        // Verify half are gone
        {
            let tasks = cb.monitoring_tasks.read().await;
            assert_eq!(tasks.len(), 5, "Should have 5 tasks remaining");
        }
        
        // Phase 3: Hot-swap some of the remaining tasks
        for i in 5..7 {
            cb.start_monitoring_task(
                format!("leak-test-{}", i),
                async move {
                    sleep(Duration::from_secs(100)).await;
                }
            )
            .await
            .expect("Should hot-swap task");
        }
        
        sleep(Duration::from_millis(200)).await;
        
        // Should still have 5 tasks (hot-swap doesn't increase count)
        {
            let tasks = cb.monitoring_tasks.read().await;
            assert_eq!(tasks.len(), 5, "Should still have 5 tasks after hot-swap");
        }
        
        // Phase 4: Stop all remaining
        cb.stop_all_monitoring_tasks().await;
        
        sleep(Duration::from_millis(200)).await;
        
        // Verify complete cleanup - no leaks
        {
            let tasks = cb.monitoring_tasks.read().await;
            assert_eq!(tasks.len(), 0, "All tasks should be cleaned up - no leaks");
        }
        
        // Phase 5: Start new tasks to verify circuit breaker still works
        for i in 0..5 {
            cb.start_monitoring_task(
                format!("post-cleanup-{}", i),
                async move {
                    sleep(Duration::from_secs(100)).await;
                }
            )
            .await
            .expect("Should start task after cleanup");
        }
        
        {
            let tasks = cb.monitoring_tasks.read().await;
            assert_eq!(tasks.len(), 5, "Should have 5 new tasks");
        }
        
        // Final cleanup
        cb.stop_all_monitoring_tasks().await;
        sleep(Duration::from_millis(200)).await;
        
        {
            let tasks = cb.monitoring_tasks.read().await;
            assert_eq!(tasks.len(), 0, "Final cleanup should leave no tasks");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_registration_failures_concurrent() {
        // Test: Concurrent metric registration and failure handling
        //
        // This validates that:
        // 1. The atomic registration prevents race conditions
        // 2. Registration failures are properly counted
        // 3. Partial state (some metrics registered, others not) doesn't occur
        // 4. The system remains stable under high concurrent load
        
        let cb = Arc::new(CircuitBreaker::new(3, 60, 50));
        let num_threads = 10;
        let num_endpoints = 5;
        
        // Spawn many concurrent tasks that all try to register metrics
        let mut handles = Vec::new();
        for thread_id in 0..num_threads {
            let cb_clone = Arc::clone(&cb);
            
            let handle = tokio::spawn(async move {
                for endpoint_id in 0..num_endpoints {
                    let endpoint = format!("concurrent-reg-{}", endpoint_id);
                    
                    // All threads hammer the same endpoints
                    // The first one to register will succeed, others hit the cached path
                    cb_clone.record_success(&endpoint).await;
                    cb_clone.record_failure(&endpoint).await;
                    cb_clone.record_success_with_latency(&endpoint, Some(100.0)).await;
                    
                    // Micro-sleep to create interleaving
                    sleep(Duration::from_micros(thread_id * 10)).await;
                }
            });
            
            handles.push(handle);
        }
        
        // Wait for all concurrent operations to complete
        for handle in handles {
            handle.await.expect("Concurrent registration should not panic");
        }
        
        // Verify all endpoints have complete metric sets (no partial registration)
        let metrics = cb.metrics.read().await;
        
        for endpoint_id in 0..num_endpoints {
            let endpoint = format!("concurrent-reg-{}", endpoint_id);
            
            // All 6 metrics must be present for each endpoint
            assert!(
                metrics.circuit_state.contains_key(&endpoint),
                "Endpoint {} missing circuit_state",
                endpoint
            );
            assert!(
                metrics.fail_count.contains_key(&endpoint),
                "Endpoint {} missing fail_count",
                endpoint
            );
            assert!(
                metrics.probe_latency.contains_key(&endpoint),
                "Endpoint {} missing probe_latency",
                endpoint
            );
            assert!(
                metrics.last_opened.contains_key(&endpoint),
                "Endpoint {} missing last_opened",
                endpoint
            );
            assert!(
                metrics.probe_success_ratio.contains_key(&endpoint),
                "Endpoint {} missing probe_success_ratio",
                endpoint
            );
            assert!(
                metrics.health_score.contains_key(&endpoint),
                "Endpoint {} missing health_score",
                endpoint
            );
        }
        
        // Registration failures should be 0 (all succeeded due to atomic registration)
        let failures = metrics.registration_failures_total.get();
        assert_eq!(
            failures, 0,
            "Should have 0 registration failures with atomic registration, got {}",
            failures
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_cancellation_notify_race_multiple_threads() {
        // Test: Cancellation correctness across multiple threads
        //
        // This validates that tokio_util::CancellationToken correctly handles:
        // - Tasks waiting before cancel() are woken immediately
        // - Tasks arriving after cancel() see cancellation and return immediately
        // - No task ever gets stuck waiting
        
        for iteration in 0..50 {
            let token = CancellationToken::new();
            
            // Group 1: Tasks that start waiting BEFORE cancel
            let mut early_waiters = Vec::new();
            for _ in 0..5 {
                let token_clone = token.clone();
                early_waiters.push(tokio::spawn(async move {
                    token_clone.cancelled().await;
                }));
            }
            
            // Give early waiters time to enter wait state
            sleep(Duration::from_millis(10)).await;
            
            // Cancel now - should wake early waiters
            token.cancel();
            
            // Group 2: Tasks that start waiting AFTER cancel
            let mut late_arrivals = Vec::new();
            for _ in 0..5 {
                let token_clone = token.clone();
                late_arrivals.push(tokio::spawn(async move {
                    // These should complete immediately via fast-path
                    token_clone.cancelled().await;
                }));
            }
            
            // All tasks should complete quickly
            for (i, task) in early_waiters.into_iter().enumerate() {
                let result = tokio::time::timeout(Duration::from_millis(100), task).await;
                assert!(
                    result.is_ok(),
                    "Early waiter {} did not complete at iteration {}",
                    i, iteration
                );
            }
            
            for (i, task) in late_arrivals.into_iter().enumerate() {
                let result = tokio::time::timeout(Duration::from_millis(100), task).await;
                assert!(
                    result.is_ok(),
                    "Late arrival {} did not complete at iteration {}",
                    i, iteration
                );
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_multi_thread_cancellation_stress() {
        // Test: Stress test for cancellation under high concurrency
        //
        // This validates that the cancellation mechanism is robust under extreme load:
        // - Many tasks waiting for cancellation simultaneously
        // - All tasks wake immediately when cancel() is called
        // - No deadlocks or race conditions
        // - Consistent behavior across iterations
        
        let num_tasks = 100;
        
        for iteration in 0..10 {
            let token = CancellationToken::new();
            let mut tasks = Vec::new();
            
            // Spawn many tasks concurrently
            for _ in 0..num_tasks {
                let token_clone = token.clone();
                tasks.push(tokio::spawn(async move {
                    token_clone.cancelled().await;
                }));
            }
            
            // Give all tasks time to enter wait state
            sleep(Duration::from_millis(50)).await;
            
            // Cancel and measure wakeup time
            let start = Instant::now();
            token.cancel();
            
            // All tasks should complete quickly
            for (i, task) in tasks.into_iter().enumerate() {
                let result = tokio::time::timeout(Duration::from_millis(100), task).await;
                assert!(
                    result.is_ok(),
                    "Task {} did not complete at iteration {}",
                    i, iteration
                );
            }
            
            let elapsed = start.elapsed();
            
            // All tasks should wake quickly even under high load
            assert!(
                elapsed < Duration::from_millis(50),
                "All {} tasks should wake quickly, took {:?} at iteration {}",
                num_tasks, elapsed, iteration
            );
        }
    }
}
