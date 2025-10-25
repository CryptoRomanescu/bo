# Time Manipulation Testing with tokio::test

## Overview

This document describes how to integrate `#[tokio::test(flavor = "multi_thread")]` with `tokio::time::pause()` for deterministic timing tests in the circuit breaker implementation.

## Background

The circuit breaker implements timing-dependent features:
- Cooldown periods after failures
- Exponential backoff
- Canary request timing
- Health score calculations over time

Testing these features with real time delays would make tests slow and potentially flaky. Tokio provides time manipulation features to test timing-dependent code deterministically.

## Prerequisites

Tokio's time manipulation features (`pause()`, `advance()`, `resume()`) are available when:
1. Using `#[tokio::test]` or `#[tokio::test(flavor = "multi_thread")]`
2. Tokio is compiled with the `test-util` feature (included in `"full"`)
3. Tests use `tokio::time::sleep()` instead of `std::thread::sleep()`

## Example: Cooldown Testing

```rust
#[tokio::test]
async fn test_cooldown_with_time_pause() {
    // Pause time - all subsequent tokio::time operations use virtual time
    tokio::time::pause();
    
    // Create circuit breaker with custom config:
    // - failure_threshold: 2
    // - cooldown_seconds: 5
    // - sample_size: 50
    // - canary_count: 3
    // - adaptive_thresholds: false
    let cb = CircuitBreaker::with_config(2, 5, 50, 3, false);
    
    // Force endpoint into cooldown
    for _ in 0..5 {
        cb.record_failure("test-endpoint").await;
    }
    
    assert_eq!(
        cb.get_endpoint_state("test-endpoint").await,
        EndpointState::CoolingDown
    );
    
    // Advance time by 3 seconds instantly (no real waiting)
    tokio::time::advance(Duration::from_secs(3)).await;
    
    // Should still be cooling down (needs more time)
    assert_eq!(
        cb.get_endpoint_state("test-endpoint").await,
        EndpointState::CoolingDown
    );
    
    // Advance past cooldown period
    tokio::time::advance(Duration::from_secs(12)).await;
    
    // Should now be half-open
    assert!(cb.is_available("test-endpoint").await);
    assert_eq!(
        cb.get_endpoint_state("test-endpoint").await,
        EndpointState::HalfOpen
    );
}
```

## Multi-threaded Time Manipulation

Time manipulation works with `#[tokio::test(flavor = "multi_thread")]`, but with caveats:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_with_time() {
    // Pause time affects all worker threads
    tokio::time::pause();
    
    let cb = Arc::new(CircuitBreaker::new(3, 60, 50));
    
    // Spawn concurrent tasks
    let mut handles = Vec::new();
    for i in 0..10 {
        let cb_clone = Arc::clone(&cb);
        handles.push(tokio::spawn(async move {
            cb_clone.record_failure(&format!("endpoint-{}", i)).await;
        }));
    }
    
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Advance time for all threads
    tokio::time::advance(Duration::from_secs(100)).await;
    
    // All endpoints should now be available (past cooldown)
    for i in 0..10 {
        assert!(cb.is_available(&format!("endpoint-{}", i)).await);
    }
}
```

## Key Points

### 1. Virtual vs Real Time

When `pause()` is active:
- `tokio::time::sleep()` uses virtual time
- `tokio::time::timeout()` uses virtual time  
- `std::thread::sleep()` still uses real time (avoid in tests!)
- `Instant::now()` uses real time (use for validation only)

### 2. Advancing Time

```rust
// Advance by a fixed duration
tokio::time::advance(Duration::from_secs(10)).await;

// Resume real time (rarely needed in tests)
tokio::time::resume();

// Re-pause after resume
tokio::time::pause();
```

### 3. Fast-Path Race Condition Testing

Time manipulation doesn't affect the cancellation token's fast-path behavior:

```rust
#[tokio::test]
async fn test_cancellation_with_time_manipulation() {
    tokio::time::pause();
    
    let token = CancellationToken::new();
    let token_wait = token.clone();
    
    let wait_task = tokio::spawn(async move {
        token_wait.cancelled().await;
    });
    
    // Advance time (doesn't affect event-driven cancellation)
    tokio::time::advance(Duration::from_secs(100)).await;
    
    // Cancel - should work immediately regardless of time
    token.cancel();
    
    // Task completes via fast-path check, not time-based
    // Using 1 second timeout to ensure reliability across different hardware.
    // The actual completion should be nearly instant due to fast-path.
    tokio::time::timeout(Duration::from_secs(1), wait_task)
        .await
        .expect("Cancellation works with time manipulation");
}
```

This validates that our cancellation mechanism is **event-driven**, not time-based, ensuring correctness even when time is manipulated. The generous timeout ensures reliability across different hardware while the fast-path check ensures actual completion is nearly instant.

## Integration with Existing Tests

The circuit breaker already has comprehensive race condition tests that use `#[tokio::test(flavor = "multi_thread")]` to validate behavior under true concurrency:

1. **test_cancel_after_await_race_condition** - Validates fast-path prevents race (100 iterations)
2. **test_ignore_cancel_task_behavior** - Tests forced abort of unresponsive tasks  
3. **test_hot_swap_race_condition** - Validates concurrent task replacement (20 swaps)
4. **test_leak_check_task_cleanup** - Validates no resource leaks through 5 phases
5. **test_cancellation_notify_race_multiple_threads** - Validates Notify semantics (50 iterations)
6. **test_multi_thread_cancellation_stress** - Stress test with 100 concurrent tasks (10 iterations)

These tests validate correctness under concurrent operations without relying on time manipulation.

## Time Manipulation Benefits

1. **Speed**: Tests run in milliseconds instead of minutes
2. **Determinism**: No race conditions from real-world timing
3. **Coverage**: Can test long cooldown periods easily
4. **Reliability**: Consistent results across different hardware

## Best Practices

1. Always use `tokio::time::pause()` at the start of time-dependent tests
2. Use `tokio::time::sleep()`, never `std::thread::sleep()`
3. Advance time in reasonable chunks (not too large)
4. Validate state after each time advance
5. Test edge cases (e.g., advancing past multiple thresholds)
6. Combine with multi-threaded tests for full coverage

## Conclusion

Time manipulation combined with multi-threaded tests provides comprehensive coverage of the circuit breaker's timing-dependent behavior while maintaining fast, deterministic test execution.

The fast-path cancellation check (`is_cancelled()` before `notified().await`) ensures correctness **independent of timing**, making the system robust under all timing scenarios - whether real time, paused time, or concurrent operations.
