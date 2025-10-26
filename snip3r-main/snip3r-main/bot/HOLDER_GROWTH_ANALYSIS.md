# Holder Growth Analysis Algorithm - Universe Grade

## Overview

The Holder Growth Analyzer is a production-grade, manipulation-resistant system for analyzing the growth patterns of unique token holders in real-time. It detects bot activity, coordinated attacks, and distinguishes organic growth from artificial manipulation.

## Architecture

### Components

1. **HolderGrowthAnalyzer** - Main analysis engine
2. **EarlyPumpCache** - TTL-based caching layer (60s default)
3. **Integration with EarlyPumpDetector** - Seamless integration into the pump detection pipeline

### Data Flow

```
Token → Snapshot Collection → Growth Analysis → Anomaly Detection → Bot Probability → Growth Score
                                                                                              ↓
                                                                                        Cache (60s TTL)
```

## Algorithm Details

### 1. Historical Snapshot Collection

The analyzer collects snapshots of holder counts at regular intervals (default: 3 seconds):

```rust
pub struct HolderSnapshot {
    timestamp: u64,           // Unix timestamp
    holder_count: usize,      // Number of unique holders
    age_seconds: u64,         // Time since token deploy
}
```

**Implementation:**
- Fetches token accounts from Solana RPC using `getProgramAccounts`
- Filters for non-zero balances
- Excludes system addresses (burn addresses, program accounts)
- Maintains rolling history per token (max 120 seconds)

### 2. Growth Rate Calculation

Calculates the average growth rate in holders per second:

```rust
growth_rate = (last_holder_count - first_holder_count) / (last_timestamp - first_timestamp)
```

### 3. Anomaly Detection

Detects four types of growth anomalies:

#### a. Sudden Jumps (Bot Coordination)

**Detection Criteria:**
- Growth rate > 5x average rate (configurable)
- Indicates coordinated bot activity

**Example:**
```
Time:     0s   3s   6s   9s   12s  15s
Holders:  10   12   14   16   150  155  ← Sudden jump at 12s
```

#### b. Suspicious Flattening (Bots Turned Off)

**Detection Criteria:**
- Consecutive snapshots with ≤5 holder change (configurable)
- Duration ≥15 seconds (configurable)
- Indicates bot activity pause

**Example:**
```
Time:     0s   3s   6s   9s   12s  15s  18s
Holders:  10   20   21   22   21   23   22  ← Flat from 6s to 18s
```

#### c. Rapid Drops (Early Dumps)

**Detection Criteria:**
- Drop rate > 3x average rate (configurable)
- Indicates holder exit/dump

#### d. Unnatural Curve (Too Linear/Smooth)

**Detection Criteria:**
- Variance in growth rates < 30% of expected variance
- Organic growth has natural variation
- Bot-driven growth is often too smooth

**Calculation:**
```rust
variance = Σ(rate_i - mean_rate)² / n
expected_variance = mean_rate * 0.3  // 30% of mean
```

### 4. Bot Probability Calculation

Each anomaly contributes to bot probability (0-1 scale):

```rust
bot_score = 0.0

For each SuddenJump:
    ratio = growth_rate / avg_rate
    excess = max(0, ratio - threshold)
    bot_score += 0.3 + (excess * 0.05)

For each SuspiciousFlattening:
    bot_score += (duration_secs / 30.0) * 0.25

For each RapidDrop:
    ratio = drop_rate / avg_rate
    excess = max(0, ratio - threshold)
    bot_score += 0.3 + (excess * 0.05)

For each UnnaturalCurve:
    ratio = variance / expected_variance
    bot_score += max(0, 1.0 - ratio) * 0.25

bot_probability = min(1.0, max(0.0, bot_score))
```

### 5. Organic Growth Classification

Growth is classified as organic if:
1. Bot probability < 0.3
2. No extreme sudden jumps (>10x average)
3. No rapid drops

```rust
is_organic = (bot_probability < 0.3) 
          && (no_extreme_jumps) 
          && (no_rapid_drops)
```

### 6. Growth Score Calculation (0-100)

Final score combines multiple factors:

```rust
score = 50.0  // Base score

// Positive factors
if is_organic:
    score += 20.0

if 0 < growth_rate < 5.0:  // Moderate growth
    score += 15.0
else if growth_rate >= 5.0:  // High growth (slightly suspicious)
    score += 5.0

score += smart_money_correlation * 15.0

// Negative factors
score -= bot_probability * 30.0
score -= wash_trading_correlation * 20.0

// Anomaly penalties
for each SuddenJump:   score -= 10.0
for each Flattening:   score -= 8.0
for each RapidDrop:    score -= 15.0
for each UnnaturalCurve: score -= 5.0

score = clamp(score, 0, 100)
```

### 7. Correlation with Other Signals

The analyzer correlates holder growth with:

- **Smart Money Involvement**: Positive correlation indicates quality
- **Wash Trading Activity**: Negative correlation indicates manipulation

## TTL Caching System

### Cache Architecture

```rust
pub struct EarlyPumpCache {
    cache: Cache<String, Arc<EarlyPumpAnalysis>>,
    metrics: Arc<Mutex<CacheMetrics>>,
    ttl_seconds: u64,  // Default: 60s
}
```

### Features

1. **Automatic TTL Expiration** (60s default)
   - Prevents stale data
   - Reduces RPC load

2. **Manual Invalidation**
   - Call when new transactions detected
   - Ensures fresh analysis on market changes

3. **Force Refresh**
   - Bypasses cache for critical decisions
   - Useful for real-time monitoring

4. **Metrics Tracking**
   - Hit/miss ratio
   - Cache size
   - Update frequency

### Cache Operations

```rust
// Check cache first
if let Some(cached) = cache.get(mint).await {
    return cached;  // Cache hit
}

// Perform analysis
let analysis = analyzer.analyze(mint, ...).await?;

// Store in cache
cache.set(mint, analysis.clone()).await;

// Invalidate on new data
cache.invalidate(mint).await;

// Force fresh analysis
cache.force_refresh(mint).await;
```

## Performance Optimizations

### 1. Efficient RPC Calls

- Single `getProgramAccounts` call per snapshot
- Filters at RPC level (data size, memcmp)
- No data slicing (minimal bandwidth)

### 2. Deduplication

- Snapshot caching per token
- Only fetch when interval elapsed (3s)

### 3. Batch Analysis

- Parallel async checks in EarlyPumpDetector
- Non-blocking snapshot collection

### 4. Memory Management

- Rolling window (max 120s of snapshots)
- Arc-based sharing for cached results

## Security Considerations

### Attack Resistance

1. **Time-based Attacks**
   - Minimum snapshot count (5) required
   - Window-based analysis (not point-in-time)

2. **Sybil Attacks**
   - Detects sudden jumps in holder count
   - Anomaly scoring penalizes coordination

3. **Wash Trading Correlation**
   - Cross-references with wash trading detector
   - Combined scoring reduces false positives

### Data Validation

- Validates mint addresses
- Filters system/burn addresses
- Handles missing/incomplete data gracefully

## Integration Example

```rust
use h_5n1p3r::oracle::{
    early_pump_detector::EarlyPumpDetector,
    holder_growth_analyzer::HolderGrowthAnalyzer,
};

// Create detector with holder growth analysis
let config = EarlyPumpConfig::default();
let rpc = Arc::new(RpcClient::new(rpc_url));
let detector = EarlyPumpDetector::new(config, rpc);

// Analyze token
let analysis = detector.analyze(
    "TokenMintAddress...",
    deploy_timestamp,
    "pump.fun"
).await?;

// Check results
println!("Holder growth score: {}", analysis.check_results.holder_growth);
println!("Decision: {:?}", analysis.decision);

// Invalidate cache on new transaction
detector.invalidate_cache("TokenMintAddress...").await;

// Force refresh
let fresh = detector.force_refresh(
    "TokenMintAddress...",
    deploy_timestamp,
    "pump.fun"
).await?;
```

## Metrics and Monitoring

### Growth Metrics

- **holder_count**: Current number of holders
- **growth_rate**: Holders per second
- **growth_score**: 0-100 quality score
- **bot_probability**: 0-1 likelihood of bot activity
- **is_organic**: Boolean organic classification

### Cache Metrics

- **hits**: Number of cache hits
- **misses**: Number of cache misses
- **hit_rate**: Percentage of requests served from cache
- **invalidations**: Number of manual invalidations
- **force_refreshes**: Number of forced refreshes

### Performance Metrics

- **analysis_time_ms**: Time to complete full analysis
- **snapshot_count**: Number of historical snapshots
- **anomaly_count**: Number of detected anomalies

## Testing Strategy

### Unit Tests

1. **Growth Rate Calculation**
   - Validates arithmetic correctness
   - Edge cases (zero holders, single snapshot)

2. **Anomaly Detection**
   - Sudden jump detection
   - Flattening detection
   - Drop detection
   - Curve smoothness

3. **Scoring Algorithm**
   - Organic token scoring (high scores)
   - Bot-driven token scoring (low scores)
   - Mixed scenarios

4. **Cache Operations**
   - TTL expiration
   - Invalidation
   - Metrics accuracy

### Integration Tests

1. **Real Token Analysis**
   - Organic memecoins
   - Bot-pumped tokens
   - Scam tokens

2. **Cache Behavior**
   - Multi-token caching
   - Concurrent access
   - Cache pressure

## Configuration

### HolderGrowthConfig

```rust
pub struct HolderGrowthConfig {
    snapshot_interval_secs: u64,          // Default: 3s
    max_analysis_window_secs: u64,        // Default: 120s
    min_snapshots: usize,                 // Default: 5
    timeout_secs: u64,                    // Default: 10s
    sudden_jump_threshold: f64,           // Default: 5x
    flattening_threshold: usize,          // Default: 5 holders
    flattening_duration_threshold: u64,   // Default: 15s
    rapid_drop_threshold: f64,            // Default: 3x
}
```

### EarlyPumpCache Config

```rust
let cache = EarlyPumpCache::new(
    1000,  // max_capacity (entries)
    60     // ttl_seconds
);
```

## Future Enhancements

### Planned Features

1. **Adaptive Thresholds**
   - Machine learning-based threshold optimization
   - Per-token-type calibration

2. **Enhanced Correlation**
   - On-chain activity correlation
   - Social sentiment correlation

3. **Predictive Analytics**
   - Growth trajectory prediction
   - Dump probability estimation

4. **Advanced Caching**
   - Tiered caching (hot/warm/cold)
   - Distributed cache support

## References

- **CryptoRomanescu Research**: Memecoin holder distribution analysis
- **CryptoLemur 2025 H2**: Median hold time analysis (100 seconds)
- **Solana SPL Token Standard**: Token account structure
- **Moka Cache**: High-performance concurrent cache

## License

This implementation is part of the H-5N1P3R trading bot system.

---

**Last Updated**: 2025-10-26
**Version**: 1.0.0
**Status**: Production Ready
