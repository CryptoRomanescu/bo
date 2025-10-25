# Bot Activity & Bundle Detection

Universe-class bot activity and bundle detection module for Solana memecoin launches.

## Overview

This module analyzes Solana token transactions to detect:
- **Bot Activity**: Automated trading with suspiciously fast response times
- **Transaction Bundling**: Coordinated groups of transactions in tight timeframes
- **Organic vs Robotic Trading**: Ratio of human vs automated trading
- **High-Frequency Patterns**: Repeated addresses executing many transactions
- **Manipulation**: Overall manipulation score combining multiple signals

## Key Statistics

Based on CryptoRomanescu analysis of memecoin launches:
- **70-85%** of first transactions are typically bots
- **>30%** organic ratio = healthy token
- **>85%** bot activity = avoid/penalize
- **Bundled transactions** indicate coordinated pumps or manipulation

## Architecture

```
BotBundleDetector
├── BotActivityAnalyzer
│   ├── Bot activity detection
│   ├── High-frequency pattern detection
│   ├── Repeated address detection
│   └── Suspicious cluster detection
├── Bundle Detection
│   ├── Coordinated bundles
│   ├── Bundle size analysis
│   └── Temporal clustering
└── Classification
    ├── Organic (>70% organic)
    ├── Mixed (30-70% organic)
    ├── BotDominated (<30% organic, <85% bots)
    ├── HighlyManipulated (>85% bots)
    └── BundleCoordinated (large coordinated bundles)
```

## Configuration

```rust
use h_5n1p3r::oracle::bot_bundle_detector::BotBundleConfig;

let config = BotBundleConfig {
    analysis_window_secs: 300,         // 5 minutes from launch
    min_transactions: 20,               // Min txs for analysis
    max_transactions: 1000,             // Max txs to analyze
    bot_timing_threshold_ms: 100,       // <100ms = likely bot
    bundle_time_window_ms: 500,         // Txs within 500ms = bundle
    min_bundle_size: 3,                 // Min txs in a bundle
    high_frequency_threshold: 10.0,     // >10 tx/s = bot
    healthy_organic_threshold: 0.30,    // 30% organic = healthy
    avoid_bot_threshold: 0.85,          // 85% bot = avoid
    analysis_timeout: Duration::from_secs(10),
};
```

## Usage

### Basic Analysis

```rust
use h_5n1p3r::oracle::bot_bundle_detector::{BotBundleConfig, BotBundleDetector};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let config = BotBundleConfig::default();
    let rpc_client = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let detector = BotBundleDetector::new(config, rpc_client);

    // Analyze a token
    let token_mint = "TokenMintAddressHere";
    let deploy_timestamp = 1234567890;
    
    let analysis = detector.analyze_token(token_mint, deploy_timestamp).await?;
    
    println!("Classification: {:?}", analysis.classification);
    println!("Bot percentage: {:.1}%", analysis.bot_percentage * 100.0);
    println!("Organic ratio: {:.1}%", analysis.organic_ratio * 100.0);
    println!("Should avoid: {}", analysis.classification.should_avoid());
}
```

### Decision Engine Integration

```rust
use h_5n1p3r::oracle::bot_bundle_detector::BotBundleScore;

// After analysis
let analysis = detector.analyze_token(token_mint, deploy_timestamp).await?;

// Get score for decision engine
let score = BotBundleScore::from_analysis(&analysis);

println!("Bot penalty: {}/100", score.bot_penalty);
println!("Bundle penalty: {}/100", score.bundle_penalty);
println!("Organic bonus: {}/100", score.organic_bonus);
println!("Manipulation score: {}/100", score.manipulation_score);
println!("Should avoid: {}", score.should_avoid);
println!("Confidence: {:.2}", score.confidence);
```

### Manual Transaction Analysis

```rust
use h_5n1p3r::oracle::bot_bundle_detector::TransactionTiming;
use chrono::Utc;

// Create transaction data
let transactions = vec![
    TransactionTiming {
        signature: "sig1".to_string(),
        address: "wallet1".to_string(),
        timestamp: Utc::now(),
        time_since_deploy_ms: 5000,
        slot: 123456,
        in_bundle: false,
        bundle_id: None,
    },
    // ... more transactions
];

// Analyze pre-fetched transactions
let analysis = detector.analyze_transactions(
    transactions,
    "token_mint",
    deploy_timestamp,
)?;
```

## Output Types

### BotBundleAnalysis

Complete analysis result containing:
- `classification`: Token classification (Organic, Mixed, BotDominated, etc.)
- `bot_percentage`: Percentage of bot activity (0.0-1.0)
- `organic_ratio`: Percentage of organic trading (0.0-1.0)
- `bot_metrics`: Detailed bot activity metrics
- `bundle_metrics`: Bundle detection metrics
- `suspicious_clusters`: Detected suspicious transaction clusters
- `repeated_addresses`: High-frequency trading addresses
- `manipulation_score`: Overall manipulation score (0.0-1.0)

### BotMetrics

- `bot_transaction_count`: Number of bot transactions
- `unique_bot_addresses`: Number of unique bot wallets
- `avg_bot_response_time_ms`: Average bot response time
- `instant_transaction_percentage`: % of instant transactions
- `high_frequency_bots`: Number of high-frequency bots

### BundleMetrics

- `bundle_count`: Number of bundles detected
- `bundled_transaction_count`: Transactions in bundles
- `bundle_percentage`: % of bundled transactions
- `max_bundle_size`: Largest bundle size
- `avg_bundle_size`: Average bundle size
- `coordinated_bundle_count`: Coordinated bundles (multiple addresses)

### BotBundleScore

Decision engine scoring:
- `bot_penalty`: 0-100 (higher = worse)
- `bundle_penalty`: 0-100 (higher = worse)
- `organic_bonus`: 0-100 (higher = better)
- `manipulation_score`: 0-100 (higher = worse)
- `should_avoid`: Boolean recommendation
- `confidence`: 0.0-1.0
- `explanation`: Human-readable explanation

## Classification Rules

### Organic
- Organic ratio > 70%
- Risk score: 0.1
- Recommendation: Safe to trade

### Mixed
- Organic ratio 30-70%
- Risk score: 0.4
- Recommendation: Proceed with caution

### BotDominated
- Organic ratio < 30% but bot % < 85%
- Risk score: 0.7
- Recommendation: High risk

### HighlyManipulated
- Bot percentage > 85%
- Risk score: 0.95
- Recommendation: **Avoid**

### BundleCoordinated
- Large coordinated bundles detected
- Coordinated bundles > 3 and bundle % > 50%
- Risk score: 1.0
- Recommendation: **Avoid**

## Performance

- **Target Latency**: <10 seconds for complete analysis
- **Max Transactions**: 1000 (configurable)
- **Analysis Complexity**: O(n log n) for most operations
- **Memory Efficient**: Streaming analysis where possible

## Testing

Run unit tests:
```bash
cargo test --lib bot_bundle_detector
```

Run integration tests:
```bash
cargo test --test test_bot_bundle_detector
```

Run example demo:
```bash
cargo run --example demo_bot_bundle_detector
```

## Integration with Decision Engine

The `BotBundleScore` type is designed for direct integration with the decision engine (issue #49):

```rust
// In your decision engine
let bot_bundle_analysis = detector.analyze_token(mint, deploy_ts).await?;
let bot_score = BotBundleScore::from_analysis(&bot_bundle_analysis);

// Incorporate into overall token score
let final_score = base_score 
    - bot_score.bot_penalty 
    - bot_score.bundle_penalty 
    + bot_score.organic_bonus;

// Use avoidance flag
if bot_score.should_avoid {
    return Decision::Pass;
}
```

## References

- **CryptoRomanescu Analysis**: Based on research showing 70-85% of first transactions in memecoin launches are bots. This analysis has been widely cited in the Solana trading community for understanding early memecoin dynamics.
- **CryptoLemur 2025 H2 Analysis**: Research indicating median memecoin hold time of approximately 100 seconds, highlighting the ultra-fast nature of memecoin trading and the importance of quick detection.
- **Nansen/Birdeye Integration Patterns**: Smart money tracking integration patterns from leading blockchain analytics platforms, used as reference for wallet behavior analysis.
- **Issue #49**: Decision Engine Integration - Provides scoring interface for integration with the main decision engine.
- **Graph Analyzer Module**: Reference implementation for transaction graph analysis and RPC integration patterns.

## Future Enhancements

- [ ] Real-time transaction streaming from Solana RPC
- [ ] Machine learning model for bot probability
- [ ] Integration with graph_analyzer for deeper analysis
- [ ] Historical pattern recognition
- [ ] Cross-token manipulation detection
- [ ] Wallet reputation scoring

## Related Modules

- `graph_analyzer`: Transaction graph analysis
- `wash_trading_detector`: Wash trading detection
- `smart_money_tracker`: Smart money monitoring
- `early_pump_detector`: Early pump detection engine
