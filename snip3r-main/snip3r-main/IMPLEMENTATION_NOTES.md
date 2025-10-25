# Circuit Breaker Implementation Notes

## What Was Implemented

This implementation adds three major enhancements to the circuit breaker system as requested in the issue:

### 1. Adaptive Thresholds with EWMA
Instead of a fixed failure threshold, the system now calculates a dynamic threshold based on:
- **EWMA (Exponentially Weighted Moving Average)** of error rates
- **Standard deviation** multiplied by sensitivity factor
- This allows distinguishing between temporary spikes and chronic degradation

**Key Formula:**
```
threshold_multiplier = EWMA_error_rate + (sensitivity × std_deviation)
adaptive_threshold = base_threshold × scaling_factor(threshold_multiplier)
```

**Configuration:**
- `ewma_alpha`: 0.3 (30% weight on recent values, 70% on historical)
- `adaptive_sensitivity`: 2.0 (2 standard deviations)
- Can be configured per instance via `with_full_config()`

### 2. Health Score System
A comprehensive health score [0, 1] calculated from three components:

1. **Error Rate (50% weight):** Based on success/failure ratio
2. **Latency (30% weight):** Normalized latency performance
   - 0-100ms: Excellent (0.95-1.0)
   - 100-1000ms: Good to Fair (0.5-0.95)
   - 1000ms+: Poor (0.0-0.5)
3. **Jitter (20% weight):** Latency stability (standard deviation)
   - Low jitter (<50ms): Stable
   - High jitter (>200ms): Unstable

**State Impact:**
- Circuit opens if health_score < min_health_score threshold
- Prevents using endpoints with acceptable error rates but terrible performance

### 3. Structured Observability

#### Prometheus Metrics (6 per endpoint)
- `circuit_breaker_state_{endpoint}`: 0=Healthy, 1=Degraded, 2=CoolingDown, 3=HalfOpen
- `circuit_breaker_failures_{endpoint}`: Cumulative failure count
- `circuit_breaker_latency_ms_{endpoint}`: Average latency
- `circuit_breaker_last_opened_{endpoint}`: Unix timestamp when opened
- `circuit_breaker_success_ratio_{endpoint}`: Success rate [0-1]
- `circuit_breaker_health_score_{endpoint}`: Health score [0-1]

#### Structured Logs
Every state transition logs:
```json
{
  "level": "INFO",
  "endpoint": "rpc.example.com",
  "state_transition": "Degraded -> Healthy",
  "reason": "success",
  "fail_count": 0,
  "success_rate": "95.00%",
  "latency_ms": 45.2
}
```

## Usage Examples

### Basic Usage (unchanged from before)
```rust
let cb = CircuitBreaker::new(5, 60, 50);
cb.record_success("endpoint").await;
cb.record_failure("endpoint").await;
```

### With Latency Tracking
```rust
let start = Instant::now();
let result = make_request(endpoint).await;
let latency = start.elapsed().as_millis() as f64;

if result.is_ok() {
    cb.record_success_with_latency(endpoint, Some(latency)).await;
} else {
    cb.record_failure_with_latency(endpoint, Some(latency)).await;
}
```

### Custom Configuration
```rust
let cb = CircuitBreaker::with_full_config(
    5,      // failure_threshold
    60,     // cooldown_seconds
    50,     // sample_size
    3,      // canary_count
    true,   // adaptive_thresholds
    0.3,    // ewma_alpha
    2.0,    // adaptive_sensitivity
    0.3,    // min_health_score
);
```

### Metrics Export
```rust
let registry = cb.get_metrics_registry().await;
let metrics = registry.gather();
// Export to Prometheus/OpenTelemetry
```

## Key Benefits

1. **Spike vs Chronic Detection:** EWMA with variance distinguishes temporary issues from persistent problems
2. **Performance-Aware:** Health score prevents using slow/unstable endpoints even if errors are low
3. **Observable:** Complete visibility into circuit state, health, and performance via metrics
4. **Configurable:** Fine-tune sensitivity and thresholds per deployment
5. **Backward Compatible:** Existing code continues to work without changes

## Testing

All 28 tests pass:
- 21 original tests (preserved existing functionality)
- 7 new tests:
  - EWMA adaptive threshold calculation
  - Health score computation
  - Health score state transitions
  - Metrics export
  - Latency tracking
  - Spike vs chronic detection
  - Configuration validation

Run tests: `cargo test circuit_breaker --lib`
Run demo: `cargo run --example demo_circuit_breaker_advanced`

## Performance Considerations

1. **Memory:** Each endpoint maintains:
   - 100 recent attempts (boolean)
   - 50 recent latencies (f64)
   - EWMA state (3 f64 values)
   - Total: ~500 bytes per endpoint

2. **Computation:** 
   - EWMA updates are O(1)
   - Health score calculation is O(n) where n=50 latencies
   - Happens only on request completion (async context)

3. **Locking:**
   - Read-heavy workload (checking availability)
   - Short write locks for updates
   - Metrics update is async

## Security Notes

- No `unwrap()` or `panic!()` in production code
- Metric names sanitized (alphanumeric only)
- Bounded data structures prevent memory growth
- Thread-safe with RwLock
- No sensitive data in logs/metrics

## Future Enhancements (Not Implemented)

The issue mentioned optional features not required for this patch:
- **Circuit mesh:** Distributed coordination across instances
- **Geolocation:** Geographic-aware routing

These can be added later without breaking changes.
