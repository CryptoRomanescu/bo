# Circuit Breaker Monitoring Task Patterns

## Overview

This document describes the **required patterns** for implementing monitoring tasks in the Circuit Breaker system. All tasks MUST be cooperatively-cancellable to ensure graceful shutdown and prevent resource leaks.

## Why Cooperatively-Cancellable Tasks?

When the circuit breaker needs to shut down monitoring tasks (e.g., during hot-swap or application shutdown), it uses a two-phase approach:

1. **Phase 1 (Graceful)**: Signal cancellation via `CancellationToken` and wait up to 5 seconds
2. **Phase 2 (Forced)**: If Phase 1 times out, forcibly abort the task via `JoinHandle::abort()` and wait up to 2 seconds

Tasks that **do not check** the `CancellationToken` or perform **blocking syscalls/FFI** will:
- Delay shutdown by up to 7 seconds (Phase 1 + Phase 2 timeouts)
- May remain stuck indefinitely if performing uninterruptible operations
- Generate warning logs and metric alerts
- Consume resources unnecessarily

## Required Pattern: tokio::select! with CancellationToken

### ✅ CORRECT Pattern

```rust
use tokio::select;
use tokio::time::{sleep, Duration};
use crate::oracle::circuit_breaker::CancellationToken;

async fn monitoring_task(cancel_token: CancellationToken) {
    info!("Monitoring task started");
    
    loop {
        select! {
            // CRITICAL: Check for cancellation on EVERY iteration
            _ = cancel_token.cancelled() => {
                info!("Received cancellation signal, performing cleanup");
                // Perform any necessary cleanup here:
                // - Flush buffers
                // - Close connections
                // - Save state
                break;
            }
            
            // Your actual work
            _ = sleep(Duration::from_secs(10)) => {
                // Do monitoring work
                check_endpoint_health().await;
            }
        }
    }
    
    info!("Monitoring task exited cleanly");
}
```

### ❌ INCORRECT Patterns

#### Pattern 1: No Cancellation Check

```rust
// BAD: Task never checks for cancellation
async fn bad_monitoring_task() {
    loop {
        sleep(Duration::from_secs(10)).await;
        check_endpoint_health().await;
        // This will delay shutdown by 7+ seconds!
    }
}
```

#### Pattern 2: Infrequent Cancellation Checks

```rust
// BAD: Only checks cancellation occasionally
async fn bad_monitoring_task(cancel_token: CancellationToken) {
    loop {
        // Do lots of work without checking cancellation
        for i in 0..100 {
            sleep(Duration::from_secs(1)).await;
            check_endpoint_health().await;
        }
        
        // Check cancellation only after 100 iterations
        if cancel_token.is_cancelled() {
            break;
        }
    }
}
```

#### Pattern 3: Blocking Operations

```rust
// BAD: Blocking operations prevent cooperative cancellation
async fn bad_monitoring_task(cancel_token: CancellationToken) {
    loop {
        select! {
            _ = cancel_token.cancelled() => break,
            
            _ = async {
                // DANGER: These blocking operations will prevent cancellation:
                
                // 1. Blocking thread sleep (use tokio::time::sleep instead)
                std::thread::sleep(Duration::from_secs(10));
                
                // 2. Blocking I/O (use tokio async I/O instead)
                let _data = std::fs::read("/some/file");
                
                // 3. Blocking FFI calls
                unsafe { some_blocking_c_function(); }
                
                // 4. CPU-intensive synchronous work
                expensive_computation_without_yielding();
            } => {
                check_endpoint_health().await;
            }
        }
    }
}
```

**Note**: Wrapping blocking operations in `async {}` does **not** make them non-blocking. 
The task will still be blocked and unable to respond to cancellation until the blocking 
operation completes. Always use async alternatives (e.g., `tokio::time::sleep`, `tokio::fs::read`).

## Advanced Patterns

### Pattern: Long-Running Operations with Periodic Checks

For tasks that perform long-running operations, split the work into smaller chunks:

```rust
async fn monitoring_task_with_long_work(cancel_token: CancellationToken) {
    loop {
        select! {
            _ = cancel_token.cancelled() => {
                info!("Cancellation received during long work");
                break;
            }
            
            _ = async {
                // Split long work into smaller chunks
                for chunk in 0..100 {
                    // Check cancellation between chunks
                    if cancel_token.is_cancelled() {
                        return;
                    }
                    
                    // Do small unit of work
                    process_chunk(chunk).await;
                }
            } => {
                info!("Long work completed");
            }
        }
    }
}
```

### Pattern: Graceful Cleanup with Timeout

For tasks that need to perform cleanup but want to avoid blocking shutdown:

```rust
use tokio::time::timeout;

async fn monitoring_task_with_cleanup(cancel_token: CancellationToken) {
    loop {
        select! {
            _ = cancel_token.cancelled() => {
                info!("Performing graceful cleanup");
                
                // Use timeout to prevent cleanup from blocking
                match timeout(Duration::from_secs(2), cleanup_resources()).await {
                    Ok(_) => info!("Cleanup completed successfully"),
                    Err(_) => warn!("Cleanup timed out, forcing exit"),
                }
                
                break;
            }
            
            _ = do_work() => {
                // Regular work
            }
        }
    }
}
```

### Pattern: External Resource Management

For tasks that manage external resources (connections, file handles, etc.):

```rust
async fn monitoring_task_with_resources(cancel_token: CancellationToken) {
    // Acquire resource
    let mut connection = connect_to_endpoint().await;
    
    loop {
        select! {
            _ = cancel_token.cancelled() => {
                info!("Closing connection gracefully");
                connection.close().await;
                break;
            }
            
            result = connection.poll() => {
                match result {
                    Ok(data) => process_data(data).await,
                    Err(e) => {
                        warn!("Connection error: {}", e);
                        break;
                    }
                }
            }
        }
    }
}
```

## Sanity Checks and Alerts

The circuit breaker monitors task behavior and will generate alerts for:

### 1. Slow Cancellation Response
- **Threshold**: Task takes >5 seconds to respond to cancellation
- **Alert**: `slow_cancellation`
- **Log Level**: WARN
- **Cause**: Task not checking `CancellationToken` frequently enough

### 2. Long-Running Tasks
- **Threshold**: Task runs for >10 seconds total
- **Alert**: `long_running_task`
- **Log Level**: WARN
- **Cause**: Task may be stuck in blocking operation or infinite loop

### 3. Phase 2 Abort Required
- **Threshold**: Task doesn't exit within 5 seconds of cancellation signal
- **Alert**: Logged in shutdown_task Phase 2
- **Log Level**: WARN
- **Cause**: Task ignoring `CancellationToken` or stuck in blocking operation

### 4. Abort Timeout
- **Threshold**: Task doesn't respond to abort within 2 seconds
- **Alert**: `abort_timeout`
- **Log Level**: WARN
- **Cause**: Task stuck in uninterruptible syscall, FFI call, or deadlock

## Metrics

The following Prometheus metrics track task health:

- `circuit_breaker_monitoring_task_count`: Total number of monitoring tasks created (counter)
- `circuit_breaker_active_tasks`: Currently active monitoring tasks (gauge)

Monitor these metrics for:
- Unexpected increases in active tasks (may indicate stuck tasks)
- Tasks not decrementing active_tasks counter (resource leak)

## Testing Your Tasks

To verify your task is cooperatively-cancellable:

```rust
#[tokio::test]
async fn test_task_cancellation() {
    let cancel_token = CancellationToken::new();
    let cancel_clone = cancel_token.clone();
    
    let handle = tokio::spawn(async move {
        your_monitoring_task(cancel_clone).await;
    });
    
    // Let task start
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Signal cancellation
    let start = Instant::now();
    cancel_token.cancel();
    
    // Verify task completes quickly
    let result = tokio::time::timeout(Duration::from_secs(1), handle).await;
    let elapsed = start.elapsed();
    
    assert!(result.is_ok(), "Task should complete within 1 second");
    assert!(elapsed < Duration::from_secs(1), "Task took too long to cancel: {:?}", elapsed);
}
```

## Summary Checklist

Before deploying a monitoring task, verify:

- [ ] Task uses `tokio::select!` with `cancel_token.cancelled()` in the loop
- [ ] Cancellation is checked on **every** iteration, not just occasionally
- [ ] No blocking syscalls or `std::thread::sleep` calls
- [ ] Long-running operations are split into cancellable chunks
- [ ] Cleanup code has timeout to prevent blocking shutdown
- [ ] Task includes proper logging for lifecycle events
- [ ] Task has been tested for timely cancellation response

## References

- Circuit Breaker Implementation: `bot/src/oracle/circuit_breaker.rs`
- CancellationToken Documentation: See inline docs in `circuit_breaker.rs`
- Task Shutdown Logic: `shutdown_task()` method with two-phase approach
