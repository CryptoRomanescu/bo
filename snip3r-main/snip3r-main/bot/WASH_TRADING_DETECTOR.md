# Wash Trading Detector

## Overview

The Wash Trading Detector is a specialized module for detecting fake volume and wash trading patterns in SPL token transactions on Solana. It uses graph analysis and transaction clustering based on CryptoLemur analysis to identify suspicious trading behavior.

## Features

- **Circular Pattern Detection**: Identifies A→B→C→A circular transaction patterns
- **Cluster Analysis**: Detects groups of wallets behaving as a single entity
- **High-Performance**: <8 second analysis per token (typically <1s)
- **Auto-Reject Flag**: Automatic flagging of extreme wash trading cases
- **Decision Engine Integration**: Seamless integration with EarlyPumpDetector

## Key Metrics

- **Wash Probability** (0.0-1.0): Overall likelihood of wash trading
- **Circularity Score** (0.0-1.0): Measure of circular transaction patterns
- **Risk Score** (0-100): Normalized risk score for decision engines
- **Suspicious Clusters**: Number of coordinated wallet clusters detected

## Usage

### Quick Start

```rust
use h_5n1p3r::oracle::graph_analyzer::{
    WashTradingScorer, TransactionGraph, GraphAnalyzerConfig,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create RPC client
    let rpc_client = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string()
    ));
    
    // Create scorer
    let scorer = WashTradingScorer::new();
    
    // Score a token
    let score = scorer.score_token("token_mint_address", rpc_client).await?;
    
    println!("Risk Score: {}/100", score.risk_score);
    println!("Wash Probability: {:.1}%", score.probability * 100.0);
    println!("Auto-Reject: {}", score.auto_reject);
    println!("Reason: {}", score.reason);
    
    Ok(())
}
```

### Advanced Usage with Custom Configuration

```rust
use h_5n1p3r::oracle::graph_analyzer::{
    WashTradingDetector, WashTradingConfig, 
    TransactionGraph, GraphAnalyzerConfig,
};

// Custom configuration
let wash_config = WashTradingConfig {
    max_analysis_time_ms: 5000,
    min_circularity: 0.3,
    min_cluster_size: 3,
    auto_reject_threshold: 0.8,
    max_cycle_length: 10,
};

let detector = WashTradingDetector::new(wash_config);

// Build graph and detect
let config = GraphAnalyzerConfig::default();
let mut graph = TransactionGraph::new(config);
// ... add transactions to graph ...

let result = detector.detect(&graph)?;

println!("Wash Probability: {:.2}", result.wash_probability);
println!("Circular Paths Found: {}", result.circular_paths.len());
println!("Suspicious Clusters: {}", result.suspicious_clusters);
```

### Integration with EarlyPumpDetector

The wash trading detector is automatically integrated into the EarlyPumpDetector:

```rust
use h_5n1p3r::oracle::early_pump_detector::{
    EarlyPumpDetector, EarlyPumpConfig,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;

let config = EarlyPumpConfig::default();
let rpc_client = Arc::new(RpcClient::new(rpc_url));
let detector = EarlyPumpDetector::new(config, rpc_client);

// Analyze token - wash trading check is included
let analysis = detector.analyze("token_mint", deploy_timestamp, "pump.fun").await?;

println!("Wash Trading Risk: {}/100", analysis.check_results.wash_trading_risk);
```

### Batch Scoring

For analyzing multiple tokens efficiently:

```rust
use h_5n1p3r::oracle::graph_analyzer::WashTradingBatchScorer;

let batch_scorer = WashTradingBatchScorer::new();
let mints = vec!["mint1", "mint2", "mint3"];

let results = batch_scorer.score_batch(&mints, rpc_client, 5).await;

for (mint, score) in results {
    println!("{}: Risk={}/100, Reason={}", mint, score.risk_score, score.reason);
}
```

## Architecture

```
┌─────────────────────────────────────────────┐
│         WashTradingDetector                  │
│  - Circular path detection                   │
│  - Cluster analysis                          │
│  - Probability calculation                   │
└──────────────┬──────────────────────────────┘
               │
       ┌───────┴────────┐
       │                │
┌──────▼──────┐  ┌─────▼────────┐
│   Scorer    │  │ EarlyPump    │
│   API       │  │ Detector     │
└─────────────┘  └──────────────┘
```

## Detection Algorithm

### 1. Circular Path Detection
- DFS-based cycle detection in transaction graph
- Identifies patterns like A→B→C→A
- Tracks repeat count and amount similarity

### 2. Cluster Identification
- Groups wallets by transaction behavior
- Calculates cohesion (internal connectivity)
- Measures internal transaction ratio

### 3. Circularity Scoring
- Measures proportion of edges in circular patterns
- Weights by repeat count and amount similarity
- Range: 0.0 (no circularity) to 1.0 (high circularity)

### 4. Probability Calculation
Combines multiple factors:
- **Circularity Score** (40% weight): Circular transaction patterns
- **Cluster Characteristics** (30% weight): Suspicious wallet clusters
- **Path Features** (30% weight): Amount similarity, repeat count

## Performance

- **Target**: <8 seconds per token
- **Typical**: <1 second for 1000 transactions
- **Graph Build**: ~100ms for 1000 transactions
- **Detection**: ~50-200ms depending on complexity

## Configuration Options

| Parameter | Default | Description |
|-----------|---------|-------------|
| `max_analysis_time_ms` | 8000 | Maximum time for analysis |
| `min_circularity` | 0.3 | Minimum circularity threshold |
| `min_cluster_size` | 3 | Minimum wallets in suspicious cluster |
| `auto_reject_threshold` | 0.8 | Probability threshold for auto-reject |
| `max_cycle_length` | 10 | Maximum cycle length to detect |

## Output Interpretation

### Risk Score (0-100)
- **0-30**: Low risk, normal trading behavior
- **31-60**: Moderate risk, some circular patterns
- **61-80**: High risk, significant wash trading
- **81-100**: Critical risk, extreme wash trading

### Auto-Reject
Triggered when `wash_probability >= auto_reject_threshold` (default 0.8)

### Reason Strings
- `"OK: Low wash trading risk"` - Safe token
- `"CAUTION: Some circular patterns detected"` - Minor concern
- `"WARNING: Moderate wash trading risk"` - Significant concern
- `"CRITICAL: High wash trading detected"` - Auto-reject

## Examples

Run the comprehensive demo:

```bash
cargo run --example demo_wash_trading_detector
```

This demonstrates:
1. Basic detection with circular patterns
2. Scoring API usage
3. Full analyzer integration

## Testing

Run all wash trading tests:

```bash
cargo test --lib wash_trading
```

Specific test categories:
```bash
# Detector tests
cargo test --lib wash_trading_detector

# API tests
cargo test --lib wash_trading_api

# Integration with pattern detector
cargo test --lib pattern_detector::test_detect_wash_trading
```

## Integration with Decision Engine

The wash trading detector integrates with the decision engine through:

1. **Risk Score**: Used in overall token scoring
2. **Auto-Reject Flag**: Immediate rejection of suspicious tokens
3. **Detailed Metrics**: Circular paths and cluster data for analysis
4. **Timing Metrics**: Performance tracking for optimization

Weight in decision engine: **10%** of total score

## References

- Based on CryptoLemur wash trading analysis
- Graph analysis techniques from research on blockchain forensics
- Circular transaction detection algorithms

## Future Enhancements

- [ ] Historical pattern learning
- [ ] Multi-token cross-analysis
- [ ] Real-time streaming updates
- [ ] Advanced graph algorithms (PageRank, centrality)
- [ ] Integration with DEX liquidity pool analytics
