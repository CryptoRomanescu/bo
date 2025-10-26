# DEX Screener Tracker - Production Deployment Guide

## Overview

This guide covers the production deployment of the DEX Screener Trending Tracker, including configuration, monitoring, error handling, and best practices.

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Configuration](#configuration)
3. [Production Setup](#production-setup)
4. [Error Handling & Resilience](#error-handling--resilience)
5. [Monitoring & Observability](#monitoring--observability)
6. [Performance Tuning](#performance-tuning)
7. [Security Considerations](#security-considerations)
8. [Troubleshooting](#troubleshooting)
9. [API Rate Limits](#api-rate-limits)

## Prerequisites

### Required

- Rust 1.70+ with async/await support
- Access to DEX Screener API
- Minimum 256MB RAM for tracker operation
- Network connectivity to `api.dexscreener.com`

### Optional

- DEX Screener API key for higher rate limits
- Prometheus for metrics collection
- Grafana for visualization
- Alert manager for notifications

## Configuration

### Basic Configuration

Add the following section to your `config.toml`:

```toml
[dex_screener]
# DEX Screener trending tracking configuration

# Base URL for DEX Screener API
base_url = "https://api.dexscreener.com/latest"

# Polling interval (must be < 4s for real-time requirement)
polling_interval_secs = 3

# Cache duration
cache_duration_secs = 2

# API timeout
api_timeout_secs = 2

# Track only top 100 trending tokens
min_trending_rank = 100

# Velocity calculation window (5 minutes)
velocity_window_secs = 300

# Enable tracking
enable_tracking = true

# Memory control - history size per token
max_history_size = 100

# Rate limiting - requests per minute
rate_limit_per_minute = 20

# Circuit breaker settings
enable_circuit_breaker = true
circuit_breaker_failure_threshold = 5
circuit_breaker_cooldown_secs = 60

# Retry configuration
max_retries = 3
fallback_to_stale_cache = true
max_stale_cache_age_secs = 60
```

### API Key Configuration

**Recommended for production:** Use environment variables for sensitive data.

```bash
# Set DEX Screener API key
export DEX_SCREENER_API_KEY="your_api_key_here"
```

The tracker will automatically use this environment variable if set.

### Advanced Configuration

#### High-Traffic Setup

For high-traffic scenarios with an API key:

```toml
[dex_screener]
polling_interval_secs = 2  # Faster polling
rate_limit_per_minute = 60  # Higher limit with API key
max_history_size = 200  # More history
```

#### Low-Resource Setup

For resource-constrained environments:

```toml
[dex_screener]
polling_interval_secs = 5  # Slower polling
rate_limit_per_minute = 10  # Conservative limit
max_history_size = 50  # Less history
min_trending_rank = 50  # Track fewer tokens
```

## Production Setup

### 1. Initialize Tracker

```rust
use h_5n1p3r::oracle::{DexScreenerConfig, DexScreenerTracker};

// Load from config.toml or use defaults
let mut config = DexScreenerConfig::default();

// Override with environment variable if set
if let Ok(api_key) = std::env::var("DEX_SCREENER_API_KEY") {
    config.api_key = Some(api_key);
}

// Create tracker
let tracker = Arc::new(DexScreenerTracker::new(config));
```

### 2. Continuous Monitoring Loop

```rust
use tokio::time::{interval, Duration};

let tracker = Arc::clone(&tracker);

tokio::spawn(async move {
    let mut interval = interval(Duration::from_secs(3));
    
    loop {
        interval.tick().await;
        
        // Get metrics for token
        match tracker.get_trending_metrics("token_address").await {
            Ok(metrics) => {
                if metrics.is_trending {
                    info!(
                        "Token trending at rank {}, velocity={:.2}",
                        metrics.current_rank.unwrap(),
                        metrics.velocity
                    );
                }
            }
            Err(e) => {
                error!("Failed to get trending metrics: {}", e);
            }
        }
    }
});
```

### 3. Event Handling

```rust
// Check for specific events
let metrics = tracker.get_trending_metrics("token_address").await?;

for event in &metrics.recent_events {
    match event {
        TrendingEvent::Entry { rank } => {
            info!("🚀 Token entered trending at rank {}", rank);
            // Trigger buy signal
        }
        TrendingEvent::Exit { final_rank } => {
            warn!("⚠️ Token exited trending from rank {}", final_rank);
            // Trigger sell signal
        }
        TrendingEvent::RankUp { old_rank, new_rank } => {
            info!("📈 Rank improved: {} → {}", old_rank, new_rank);
        }
        TrendingEvent::RankDown { old_rank, new_rank } => {
            warn!("📉 Rank declined: {} → {}", old_rank, new_rank);
        }
        TrendingEvent::HighMomentum { velocity } => {
            info!("⚡ High momentum detected: {:.2} ranks/min", velocity);
        }
    }
}
```

## Error Handling & Resilience

### Circuit Breaker

The circuit breaker prevents cascading failures when the API is unhealthy:

- **Closed (Normal)**: All requests pass through
- **Open (Failing)**: No requests sent, immediate fallback
- **Half-Open (Testing)**: Limited requests to test recovery

**Configuration:**

```toml
enable_circuit_breaker = true
circuit_breaker_failure_threshold = 5  # Open after 5 consecutive failures
circuit_breaker_cooldown_secs = 60     # Wait 60s before retrying
```

### Retry Logic

Automatic retries with exponential backoff:

1. First retry: 100ms delay
2. Second retry: 200ms delay
3. Third retry: 400ms delay

**Configuration:**

```toml
max_retries = 3  # Try up to 3 times before failing
```

### Fallback Strategy

When API fails, the tracker falls back to cached data:

```toml
fallback_to_stale_cache = true
max_stale_cache_age_secs = 60  # Use cache up to 60s old
```

**Behavior:**

1. API request fails
2. Check circuit breaker state
3. If cache available and fresh enough, use it
4. Log warning about using stale data
5. Continue operation

### Error Logging

All errors are logged with structured fields:

```rust
error!(
    error = %e,
    consecutive_failures = failure_count,
    "Failed to poll DEX Screener API"
);
```

**Log Levels:**

- `ERROR`: API failures, critical issues
- `WARN`: Circuit breaker open, stale cache usage
- `INFO`: Successful polls, trending events
- `DEBUG`: Detailed operation logs

## Monitoring & Observability

### Key Metrics to Monitor

1. **API Health**
   - Request success rate
   - Response latency
   - Circuit breaker state
   - Consecutive failures

2. **Trending Data**
   - Number of tokens tracked
   - Event detection rate
   - Cache hit rate
   - Data freshness

3. **Resource Usage**
   - Memory consumption (history size)
   - CPU usage
   - Network bandwidth

### Prometheus Metrics

The circuit breaker exposes Prometheus metrics:

```
# Circuit breaker state
circuit_breaker_state{endpoint="dex_screener_api"} 0  # 0=Healthy

# Consecutive failures
circuit_breaker_consecutive_failures{endpoint="dex_screener_api"} 0

# Total failures
circuit_breaker_failures_total{endpoint="dex_screener_api"} 5

# Success rate
circuit_breaker_success_ratio{endpoint="dex_screener_api"} 0.98
```

### Health Checks

Implement health check endpoints:

```rust
async fn health_check(tracker: Arc<DexScreenerTracker>) -> Result<HealthStatus> {
    // Check if tracker can fetch data
    let is_healthy = tracker.is_trending("test").await;
    
    // Check circuit breaker state
    let cb_state = if let Some(ref cb) = tracker.circuit_breaker {
        cb.is_available("dex_screener_api").await
    } else {
        true
    };
    
    Ok(HealthStatus {
        healthy: is_healthy && cb_state,
        details: "DEX Screener tracker operational"
    })
}
```

## Performance Tuning

### Memory Optimization

Control memory usage with history limits:

```toml
max_history_size = 100  # 100 positions + 100 events per token
```

**Memory Calculation:**

- Per position: ~300 bytes
- Per event: ~400 bytes
- Per token: ~70 KB (with max_history_size=100)
- 100 tokens: ~7 MB

**Recommendations:**

- Small deployments: 50-100 history size
- Medium deployments: 100-200 history size
- Large deployments: 200-500 history size

### Rate Limiting

Optimize for your API tier:

| API Tier | Rate Limit | Config Value |
|----------|-----------|--------------|
| Free | 5/min | `rate_limit_per_minute = 5` |
| Basic | 20/min | `rate_limit_per_minute = 20` |
| Pro | 60/min | `rate_limit_per_minute = 60` |
| Enterprise | 300/min | `rate_limit_per_minute = 300` |

### Polling Interval

Balance latency vs. load:

```toml
# Fast (high load): <4s latency requirement
polling_interval_secs = 2

# Normal (balanced)
polling_interval_secs = 3

# Slow (low load)
polling_interval_secs = 5
```

## Security Considerations

### 1. API Key Protection

**Never commit API keys to version control!**

```bash
# Use environment variables
export DEX_SCREENER_API_KEY="sk_live_..."

# Or use secrets management
kubectl create secret generic dex-screener \
  --from-literal=api-key="sk_live_..."
```

### 2. Rate Limit Protection

The rate limiter prevents accidental DDoS:

```rust
// Automatically waits if rate limit exceeded
tracker.get_trending_metrics("token").await?;  // Safe
```

### 3. Input Validation

All token addresses are validated:

```rust
// Invalid addresses are logged and skipped
let metrics = tracker.get_trending_metrics("invalid").await?;
```

### 4. Network Security

**Recommended:** Use TLS for all API requests (enabled by default).

```rust
// Uses HTTPS by default
base_url = "https://api.dexscreener.com/latest"
```

## Troubleshooting

### Issue: Circuit Breaker Constantly Open

**Symptoms:**
- Logs show "Circuit breaker open" warnings
- No new trending data being fetched

**Solutions:**

1. Check API connectivity:
   ```bash
   curl https://api.dexscreener.com/latest/dex/search?q=solana
   ```

2. Verify API key (if using):
   ```bash
   curl -H "X-API-KEY: your_key" https://api.dexscreener.com/latest/dex/search?q=solana
   ```

3. Adjust circuit breaker thresholds:
   ```toml
   circuit_breaker_failure_threshold = 10  # More tolerant
   circuit_breaker_cooldown_secs = 30      # Faster recovery
   ```

### Issue: High Memory Usage

**Symptoms:**
- Memory usage growing unbounded
- OOM errors

**Solutions:**

1. Reduce history size:
   ```toml
   max_history_size = 50  # Smaller history
   ```

2. Limit tracked tokens:
   ```toml
   min_trending_rank = 50  # Track fewer tokens
   ```

3. Monitor memory with:
   ```rust
   let positions = tracker.trending_positions.read().await;
   info!("Tracking {} tokens", positions.len());
   ```

### Issue: Rate Limit Exceeded

**Symptoms:**
- 429 Too Many Requests errors
- Rate limiter blocking requests

**Solutions:**

1. Reduce polling frequency:
   ```toml
   polling_interval_secs = 5  # Slower polling
   ```

2. Lower rate limit:
   ```toml
   rate_limit_per_minute = 10  # Conservative limit
   ```

3. Get an API key for higher limits

### Issue: Stale Data

**Symptoms:**
- Old trending data
- "Using stale cache" warnings

**Solutions:**

1. Check API availability
2. Verify network connectivity
3. Increase cache age tolerance:
   ```toml
   max_stale_cache_age_secs = 120  # Allow older cache
   ```

## API Rate Limits

### DEX Screener Limits (Free Tier)

- **Rate:** 5 requests per minute
- **Burst:** Up to 10 requests
- **Daily:** 500 requests per day

### With API Key

- **Rate:** 20-300 requests per minute (depending on tier)
- **Burst:** Higher burst allowance
- **Daily:** Higher or unlimited

### Recommendations

1. **Without API Key:**
   ```toml
   polling_interval_secs = 12  # Stay under 5/min
   rate_limit_per_minute = 5
   ```

2. **With Basic API Key:**
   ```toml
   polling_interval_secs = 3
   rate_limit_per_minute = 20
   ```

3. **With Pro API Key:**
   ```toml
   polling_interval_secs = 2
   rate_limit_per_minute = 60
   ```

## Best Practices

### 1. Graceful Degradation

Always implement fallback behavior:

```rust
match tracker.get_trending_metrics("token").await {
    Ok(metrics) => process_metrics(metrics),
    Err(e) => {
        warn!("Using default metrics: {}", e);
        TrendingMetrics::default()
    }
}
```

### 2. Error Budgets

Set acceptable error rates:

- **Target:** 99.9% success rate
- **Circuit breaker:** Opens at 95% failure rate
- **Alerts:** Trigger at 98% success rate

### 3. Staged Rollout

Deploy in stages:

1. **Development:** Full logging, low rate limits
2. **Staging:** Production config, high logging
3. **Production:** Optimized config, structured logging

### 4. Monitoring Alerts

Set up alerts for:

- Circuit breaker open > 5 minutes
- Success rate < 98%
- Memory usage > 80%
- Consecutive failures > 10

## Example Production Setup

Complete production configuration:

```toml
[dex_screener]
# Production DEX Screener configuration
base_url = "https://api.dexscreener.com/latest"

# Polling - balanced for production
polling_interval_secs = 3
cache_duration_secs = 2
api_timeout_secs = 2

# Trending criteria
min_trending_rank = 100
velocity_window_secs = 300

# Control flags
enable_tracking = true

# Memory management
max_history_size = 100

# Rate limiting (adjust based on API tier)
rate_limit_per_minute = 20

# Circuit breaker - production settings
enable_circuit_breaker = true
circuit_breaker_failure_threshold = 5
circuit_breaker_cooldown_secs = 60

# Retry and fallback
max_retries = 3
fallback_to_stale_cache = true
max_stale_cache_age_secs = 60
```

```bash
# Environment variables
export DEX_SCREENER_API_KEY="sk_live_..."
export LOG_LEVEL="info"
export RUST_LOG="h_5n1p3r::oracle::dex_screener_tracker=debug"
```

## Support & Resources

- **DEX Screener API Docs:** https://docs.dexscreener.com/
- **Issue Tracker:** GitHub Issues
- **Monitoring:** Prometheus + Grafana dashboards
- **Community:** Discord server

## Changelog

### Version 1.0.0 (Production Ready)

- ✅ Circuit breaker integration
- ✅ Rate limiting with governor
- ✅ Automatic retry with exponential backoff
- ✅ Fallback to stale cache
- ✅ Configurable via config.toml
- ✅ Memory-controlled history
- ✅ Comprehensive error handling
- ✅ Production-grade logging
- ✅ 30 comprehensive tests

## License

See main project LICENSE file.
