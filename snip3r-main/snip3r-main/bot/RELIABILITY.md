# Reliability & Circuit Breaker Configuration

This document describes the advanced circuit breaker pattern implementation for ensuring system reliability and graceful degradation of services.

## Overview

The circuit breaker pattern protects the system from cascading failures by temporarily blocking requests to unhealthy endpoints. Our implementation features:

- **Adaptive Thresholds**: Dynamically adjusts failure thresholds based on live error rates
- **Canary Requests**: Tests endpoint recovery with a small number of requests (half-open state)
- **Exponential Backoff with Jitter**: Prevents thundering herd problems during recovery
- **Per-Endpoint Isolation**: Each endpoint has its own circuit breaker state
- **Real-Time Monitoring**: State change callbacks and structured logging for observability
- **Configurable Behavior**: Flexible configuration for different use cases

## Circuit Breaker States

The circuit breaker can be in one of four states:

### 1. Healthy (Closed)
- **Description**: Endpoint is functioning normally
- **Behavior**: All requests are allowed
- **Transition**: Moves to Degraded after reaching failure threshold

### 2. Degraded
- **Description**: Endpoint is experiencing issues but still usable
- **Behavior**: Requests are allowed but endpoint is deprioritized
- **Transition**: 
  - To Healthy: When consecutive failures reset to 0 and success rate > 70%
  - To CoolingDown: When failures reach 2× threshold or success rate < 30%

### 3. CoolingDown (Open)
- **Description**: Endpoint is temporarily blocked due to excessive failures
- **Behavior**: No requests are allowed during cooldown period
- **Transition**: To HalfOpen after cooldown period expires

### 4. HalfOpen
- **Description**: Testing endpoint recovery with canary requests
- **Behavior**: Limited number of test requests are allowed
- **Transition**:
  - To Degraded: If canary success rate ≥ 66%
  - To CoolingDown: If canary tests fail (with increased backoff)

## Configuration

### Basic Configuration

```rust
use h_5n1p3r::oracle::CircuitBreaker;

// Create with default settings
let circuit_breaker = CircuitBreaker::new(
    5,      // failure_threshold: failures before degradation
    60,     // cooldown_seconds: cooldown duration
    50      // sample_size: window size for success rate calculation
);
```

### Advanced Configuration

```rust
use h_5n1p3r::oracle::CircuitBreaker;

// Create with custom settings
let circuit_breaker = CircuitBreaker::with_config(
    10,     // failure_threshold
    30,     // cooldown_seconds
    100,    // sample_size
    5,      // canary_count: number of canary requests
    true    // adaptive_thresholds: enable/disable adaptive behavior
);
```

### Configuration Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `failure_threshold` | `u32` | 5 | Number of consecutive failures before marking endpoint as degraded |
| `cooldown_seconds` | `u64` | 60 | Duration to wait before attempting recovery |
| `sample_size` | `usize` | 50 | Number of recent requests to consider for success rate |
| `canary_count` | `u32` | 3 | Number of test requests in half-open state |
| `adaptive_thresholds` | `bool` | true | Enable dynamic threshold adjustment based on error rates |
| `min_success_rate` | `f64` | 0.3 | Minimum success rate to avoid cooldown (30%) |
| `canary_success_threshold` | `f64` | 0.66 | Required success rate for canary tests (66%) |
| `max_backoff` | `u32` | 32 | Maximum backoff multiplier for exponential backoff |

## Adaptive Thresholds

When enabled, the circuit breaker automatically adjusts failure thresholds based on the current error rate:

- **High Error Rate (>50%)**: Threshold reduced to 50% of configured value (more sensitive)
- **Medium Error Rate (30-50%)**: Threshold reduced to 75% of configured value
- **Low Error Rate (<30%)**: Threshold remains at configured value

This ensures the system responds quickly to deteriorating conditions while being tolerant of occasional errors.

## Exponential Backoff with Jitter

Failed recovery attempts trigger exponential backoff:

- Initial backoff: `base_backoff_seconds × 1`
- After 1st failed canary: `base_backoff_seconds × 2`
- After 2nd failed canary: `base_backoff_seconds × 4`
- Maximum: `base_backoff_seconds × 32`

**Jitter** (random variation between 80-120%) prevents multiple endpoints from retrying simultaneously, avoiding thundering herd problems.

## Real-Time Monitoring

### State Change Callbacks

Register a callback to receive notifications when circuit breaker states change:

```rust
circuit_breaker.set_state_change_callback(|endpoint, old_state, new_state| {
    println!("Circuit breaker for {} changed: {:?} -> {:?}", 
             endpoint, old_state, new_state);
    // Send alert, update metrics, etc.
});
```

### Structured Logging

The circuit breaker emits structured log events using the `tracing` framework:

- **ERROR level**: When circuit opens (enters CoolingDown)
- **WARN level**: When entering HalfOpen state
- **INFO level**: When circuit closes (fully recovered) or state changes
- **DEBUG level**: For detailed state transitions and operations

Example log output:
```
ERROR circuit_breaker: Circuit breaker OPENED - endpoint unavailable 
  endpoint="https://api.example.com" old_state=Degraded new_state=CoolingDown
  
WARN circuit_breaker: Circuit breaker HALF-OPEN - testing with canary requests
  endpoint="https://api.example.com" old_state=CoolingDown new_state=HalfOpen
  
INFO circuit_breaker: Circuit breaker CLOSED - endpoint fully recovered
  endpoint="https://api.example.com" old_state=Degraded new_state=Healthy
```

### Health Statistics

Query circuit breaker health for all endpoints:

```rust
let stats = circuit_breaker.get_health_stats();
for (endpoint, health) in stats {
    println!("{}: state={:?}, failures={}, success_rate={:.2}%",
             endpoint,
             health.state,
             health.consecutive_failures,
             health.success_rate * 100.0);
             
    if let Some((successes, total)) = health.canary_progress {
        println!("  Canary progress: {}/{}", successes, total);
    }
}
```

## Usage Examples

### Basic Usage

```rust
use h_5n1p3r::oracle::{CircuitBreaker, EndpointState};

let mut cb = CircuitBreaker::new(5, 60, 50);

// Check if endpoint is available
if cb.is_available("https://api.example.com") {
    // Make request
    match make_request("https://api.example.com") {
        Ok(_) => cb.record_success("https://api.example.com"),
        Err(_) => cb.record_failure("https://api.example.com"),
    }
}
```

### With Monitoring

```rust
use std::sync::{Arc, Mutex};
use h_5n1p3r::oracle::CircuitBreaker;

// Create shared circuit breaker
let cb = Arc::new(Mutex::new(CircuitBreaker::new(5, 60, 50)));
let cb_clone = Arc::clone(&cb);

// Register state change callback
cb.lock().unwrap().set_state_change_callback(move |endpoint, old_state, new_state| {
    // Send alert to monitoring system
    send_alert(&format!(
        "Circuit breaker for {} changed from {:?} to {:?}",
        endpoint, old_state, new_state
    ));
});

// Use in request handler
async fn handle_request(endpoint: &str) -> Result<Response, Error> {
    let mut circuit_breaker = cb_clone.lock().unwrap();
    
    if !circuit_breaker.is_available(endpoint) {
        return Err(Error::ServiceUnavailable);
    }
    
    match make_request(endpoint).await {
        Ok(response) => {
            circuit_breaker.record_success(endpoint);
            Ok(response)
        }
        Err(e) => {
            circuit_breaker.record_failure(endpoint);
            Err(e)
        }
    }
}
```

### Per-Endpoint Configuration

```rust
// Different endpoints can have different circuit breakers
let rpc_breaker = CircuitBreaker::new(3, 30, 30);  // Sensitive, quick recovery
let api_breaker = CircuitBreaker::new(10, 120, 100); // Tolerant, slow recovery
```

## Best Practices

### 1. Choose Appropriate Thresholds
- **Critical services**: Lower threshold (3-5), shorter cooldown (30-60s)
- **Optional services**: Higher threshold (10-15), longer cooldown (120-300s)

### 2. Monitor Circuit Breaker Metrics
Track:
- Frequency of state transitions
- Time spent in each state
- Canary test success rates
- Backoff multipliers

### 3. Test Recovery Scenarios
- Simulate endpoint failures
- Verify canary requests work correctly
- Ensure backoff prevents rapid retries

### 4. Combine with Rate Limiting
Circuit breakers work best alongside rate limiting:
- Rate limiting: Prevents overwhelming healthy services
- Circuit breakers: Prevents requests to unhealthy services

### 5. Use State Change Callbacks
Implement callbacks to:
- Send alerts to on-call engineers
- Update dashboards in real-time
- Log to centralized monitoring systems
- Trigger automated remediation

## Troubleshooting

### Endpoint Stuck in CoolingDown
**Cause**: Canary tests repeatedly fail
**Solution**: 
- Check if endpoint is actually healthy
- Verify network connectivity
- Review backoff multiplier (may need reset)
- Consider increasing canary_success_threshold

### Too Many False Positives
**Cause**: Threshold too sensitive
**Solution**:
- Increase failure_threshold
- Increase sample_size for better averaging
- Disable adaptive_thresholds if causing issues

### Circuit Never Opens
**Cause**: Threshold too permissive or success rate check too lenient
**Solution**:
- Decrease failure_threshold
- Increase min_success_rate
- Enable adaptive_thresholds

## Integration with Existing Systems

The circuit breaker is integrated into the Oracle system for RPC endpoint health management:

```rust
// In quantum_oracle.rs (example integration)
use h_5n1p3r::oracle::CircuitBreaker;

struct QuantumOracle {
    circuit_breaker: Arc<Mutex<CircuitBreaker>>,
    // ... other fields
}

impl QuantumOracle {
    async fn fetch_from_rpc(&self, endpoint: &str) -> Result<Data, Error> {
        let mut cb = self.circuit_breaker.lock().unwrap();
        
        if !cb.is_available(endpoint) {
            return Err(Error::CircuitOpen);
        }
        
        match self.rpc_client.get(endpoint).await {
            Ok(data) => {
                cb.record_success(endpoint);
                Ok(data)
            }
            Err(e) => {
                cb.record_failure(endpoint);
                Err(e)
            }
        }
    }
}
```

## Future Enhancements

Planned improvements:
- [ ] Integration with Prometheus metrics
- [ ] Distributed circuit breaker state (Redis/etcd)
- [ ] Machine learning for adaptive threshold tuning
- [ ] Circuit breaker presets for common scenarios
- [ ] WebSocket support for real-time state updates

## References

- [Martin Fowler - Circuit Breaker Pattern](https://martinfowler.com/bliki/CircuitBreaker.html)
- [Microsoft - Circuit Breaker Pattern](https://docs.microsoft.com/en-us/azure/architecture/patterns/circuit-breaker)
- [Release It! by Michael Nygard](https://pragprog.com/titles/mnee2/release-it-second-edition/)
