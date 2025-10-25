# Actor System Foundation

This document describes the actor-based architecture implemented for the H-5N1P3R trading bot, providing scalability, fault tolerance, and clean separation of concerns.

## Overview

The actor system is built on top of the [actix](https://actix.rs/) actor framework, which provides:

- **Isolation**: Each actor has its own state and mailbox
- **Asynchronous messaging**: Non-blocking communication between components
- **Fault tolerance**: Supervision strategies for automatic restart on failure
- **Scalability**: Easy to distribute across threads or machines

## Architecture

```
┌─────────────────────────────────────────────┐
│          SupervisorActor                     │
│  - Manages actor lifecycle                   │
│  - Implements restart strategies             │
│  - Monitors system health                    │
└──────────┬──────────┬──────────┬─────────────┘
           │          │          │
    ┌──────▼──┐  ┌───▼─────┐  ┌─▼────────┐
    │ Oracle  │  │ Storage │  │ Monitor  │
    │ Actor   │  │ Actor   │  │ Actor    │
    └─────────┘  └─────────┘  └──────────┘
```

## Core Components

### 1. OracleActor

**Purpose**: Wraps the PredictiveOracle component for candidate scoring.

**Messages**:
- `ScoreCandidate`: Send a candidate for scoring
- `UpdateOracleConfig`: Update weights/thresholds dynamically
- `GetOracleMetrics`: Query current metrics

**Example**:
```rust
let oracle_actor = OracleActor::new(config, scored_sender)?;
let addr = oracle_actor.start();

// Score a candidate
addr.send(ScoreCandidate { candidate }).await?;

// Get metrics
let metrics = addr.send(GetOracleMetrics).await?;
```

### 2. StorageActor

**Purpose**: Wraps the DecisionLedger for persistent storage operations.

**Messages**:
- `RecordDecision`: Store a new decision record
- `UpdateOutcome`: Update transaction outcome
- `GetStorageStats`: Query storage statistics

**Example**:
```rust
let storage_actor = StorageActor::new().await?;
let addr = storage_actor.start();

// Record a decision
addr.send(RecordDecision { record }).await?;

// Get stats
let stats = addr.send(GetStorageStats).await?;
```

### 3. MonitorActor

**Purpose**: Wraps the TransactionMonitor for tracking transaction outcomes.

**Messages**:
- `MonitorTransaction`: Add a transaction to monitoring queue
- `GetMonitorStats`: Query monitoring statistics

**Example**:
```rust
let monitor_actor = MonitorActor::new(storage, update_sender, rpc_url, wallet_pubkey, interval);
let addr = monitor_actor.start();

// Monitor a transaction
addr.send(MonitorTransaction { transaction }).await?;
```

### 4. SupervisorActor

**Purpose**: Manages all other actors with fault tolerance.

**Messages**:
- `GetSystemHealth`: Check if all actors are running
- `ShutdownSystem`: Graceful shutdown of all actors

**Supervision Strategies**:
- `RestartImmediately`: Restart failed actors instantly
- `RestartWithDelay(duration)`: Wait before restarting
- `ExponentialBackoff`: Use exponential backoff (100ms, 200ms, 400ms, ..., max 5s)

**Example**:
```rust
let supervisor = SupervisorActor::new(
    SupervisionStrategy::ExponentialBackoff,
    oracle_config,
    rpc_url,
    wallet_pubkey,
    check_interval_ms,
    scored_sender,
);
let addr = supervisor.start();

// Check system health
let health = addr.send(GetSystemHealth).await?;
```

## Message Protocol Design

All messages implement the `actix::Message` trait and define their result type:

```rust
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct ScoreCandidate {
    pub candidate: PremintCandidate,
}
```

Message handlers are implemented for each actor:

```rust
impl Handler<ScoreCandidate> for OracleActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;
    
    fn handle(&mut self, msg: ScoreCandidate, _ctx: &mut Context<Self>) -> Self::Result {
        // Handle the message asynchronously
    }
}
```

## Fault Tolerance

The SupervisorActor monitors all child actors and automatically restarts them if they crash:

1. **Detection**: Actor stops or panics
2. **Strategy Selection**: Choose restart delay based on strategy
3. **Restart**: Create new actor instance
4. **State Recovery**: Actors recover from persistent storage

### Restart Statistics

The supervisor tracks restart counts for each actor type:
- Oracle restarts
- Storage restarts
- Monitor restarts
- Last restart timestamp

## Migration Guide

### From Direct Components to Actors

**Before**:
```rust
let oracle = PredictiveOracle::new(receiver, sender, config)?;
tokio::spawn(async move {
    oracle.score_candidate(&candidate).await?;
});
```

**After**:
```rust
let oracle_actor = OracleActor::new(config, sender)?;
let addr = oracle_actor.start();
addr.send(ScoreCandidate { candidate }).await?;
```

### Benefits

1. **Isolation**: Actor failures don't affect other components
2. **Supervision**: Automatic restart on failure
3. **Testability**: Easy to test with mock messages
4. **Scalability**: Can distribute actors across machines

## Testing

### Unit Tests

Test individual actors:

```rust
#[actix::test]
async fn test_oracle_actor_creation() {
    let oracle_actor = OracleActor::new(config, sender)?;
    assert!(oracle_actor.is_ok());
}
```

### Integration Tests

Test message passing:

```rust
#[actix::test]
async fn test_oracle_metrics() {
    let addr = oracle_actor.start();
    let metrics = addr.send(GetOracleMetrics).await?;
    assert_eq!(metrics.total_scored, 0);
}
```

### System Tests

Test supervision:

```rust
#[actix::test]
async fn test_get_system_health() {
    let supervisor_addr = supervisor.start();
    let health = supervisor_addr.send(GetSystemHealth).await?;
    assert!(health.oracle_healthy);
}
```

## Performance Considerations

### Message Queue Sizing

All actors use buffered channels with configurable sizes:
- Default: 100 messages
- High throughput: 1000+ messages
- Low latency: 10-50 messages

### Actor Lifecycle

- **Startup**: O(1) - minimal overhead
- **Message processing**: Async, non-blocking
- **Shutdown**: Graceful, waits for pending messages

### Memory Usage

- Each actor: ~1-2KB overhead
- Message queue: Buffer size × message size
- Total system: <10KB for 3 actors

## Future Enhancements

1. **Remote Actors**: Distribute across network
2. **Persistent State**: Save actor state on crash
3. **Load Balancing**: Multiple actor instances
4. **Metrics Collection**: Prometheus integration
5. **Dynamic Scaling**: Add/remove actors at runtime

## References

- [Actix Documentation](https://actix.rs/docs/)
- [Actor Model Pattern](https://en.wikipedia.org/wiki/Actor_model)
- Issue: [Foundation] Implement Actor System Foundation

## Example

See `examples/demo_actor_system.rs` for a complete working example:

```bash
cargo run --example demo_actor_system
```

This demonstrates:
- Creating and starting actors
- Sending messages
- Querying system health
- Graceful shutdown
