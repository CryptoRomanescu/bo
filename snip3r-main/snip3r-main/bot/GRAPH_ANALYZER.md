# On-Chain Graph Analyzer

## Overview

The On-Chain Graph Analyzer detects pump & dump schemes, wash trading, and wallet clustering patterns by analyzing Solana transaction graphs.

## Features

- **Transaction Graph Construction**: Builds directed graphs from Solana RPC data
- **Pump & Dump Detection**: Identifies coordinated buying followed by large sell-offs
- **Wash Trading Detection**: Detects circular transaction patterns (A→B→C→A)
- **Wallet Clustering**: Groups related wallets based on transaction patterns
- **Sybil Attack Detection**: Identifies clusters of wallets controlled by the same entity
- **Real-time Updates**: Supports incremental graph updates for live monitoring
- **ML Integration**: Provides 16 graph-based features for predictive models

## Performance

- **Build Time**: < 100ms for 1000 transactions, well under 5 seconds for 10k wallets
- **Memory Usage**: ~540 bytes per wallet node, ~5.4MB for 10k wallets
- **Detection Accuracy**: Designed for >90% accuracy on known pump patterns

## Usage

### Basic Usage

```rust
use h_5n1p3r::oracle::graph_analyzer::{OnChainGraphAnalyzer, GraphAnalyzerConfig};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // Create RPC client
    let rpc_client = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string()
    ));

    // Configure analyzer
    let config = GraphAnalyzerConfig {
        max_wallets: 10_000,
        max_transactions: 10_000,
        min_confidence: 0.7,
        time_window_secs: 3600,
        ..Default::default()
    };

    // Create analyzer
    let mut analyzer = OnChainGraphAnalyzer::new(config, rpc_client);

    // Analyze a token
    let result = analyzer.analyze_token("TokenMintAddress").await.unwrap();

    println!("Detected {} patterns", result.patterns.len());
    println!("Risk score: {:.2}", result.summary.risk_score);
    println!("Is suspicious: {}", result.summary.is_suspicious);
}
```

### Real-time Updates

```rust
use chrono::Utc;

// Add new transaction in real-time
let edge = TransactionEdge {
    signature: "transaction_signature".to_string(),
    amount: 5_000_000_000,
    timestamp: Utc::now(),
    slot: 123456,
    token_mint: Some("token_mint".to_string()),
};

analyzer.update_with_transaction("from_wallet", "to_wallet", edge)?;

// Re-run pattern detection
let patterns = analyzer.get_patterns_by_type(PatternType::PumpAndDump);
```

### Feature Extraction for ML

```rust
use h_5n1p3r::oracle::feature_engineering::GraphAnalysisExtractor;

// Create extractor
let extractor = GraphAnalysisExtractor::new(rpc_client);

// Extract features asynchronously
let features = extractor.extract_async(&candidate, &token_data).await?;

// Use features in ML model
println!("Risk score: {}", features.get("graph_risk_score").unwrap());
```

## Configuration

```rust
pub struct GraphAnalyzerConfig {
    /// Maximum number of transactions to analyze per token
    pub max_transactions: usize,         // Default: 10,000
    
    /// Time window for pattern detection (in seconds)
    pub time_window_secs: i64,           // Default: 3600 (1 hour)
    
    /// Minimum confidence threshold for pattern detection
    pub min_confidence: f64,             // Default: 0.7
    
    /// Maximum wallets to track in graph
    pub max_wallets: usize,              // Default: 10,000
    
    /// Enable real-time updates
    pub enable_realtime_updates: bool,   // Default: true
    
    /// Pump detection threshold (price increase ratio)
    pub pump_threshold: f64,             // Default: 5.0 (5x)
    
    /// Wash trading detection threshold (circular tx ratio)
    pub wash_trading_threshold: f64,     // Default: 0.3 (30%)
}
```

## Pattern Types

### Pump and Dump
Detects coordinated buying followed by large sell-offs:
- Temporal clustering: Multiple wallets buying at similar times
- Volume spike: Unusual transaction volume
- Price manipulation: Rapid price increases

### Wash Trading
Detects circular transaction patterns:
- Circular paths: A→B→C→A patterns
- Similar amounts: Transactions with similar values
- Rapid succession: Quick consecutive trades

### Coordinated Buying
Detects groups of wallets buying together:
- Coordination score: Inter-wallet transaction patterns
- Temporal clustering: Synchronized buying behavior

### Sybil Clusters
Detects groups of wallets controlled by one entity:
- High cohesion: Tight interconnection between wallets
- Similar behavior: Synchronized transaction patterns

## ML Features

The graph analyzer provides 16 features for machine learning models:

| Feature | Description |
|---------|-------------|
| `graph_pump_dump_count` | Number of pump & dump patterns detected |
| `graph_wash_trading_count` | Number of wash trading patterns detected |
| `graph_coordinated_buying_count` | Number of coordinated buying patterns |
| `graph_sybil_cluster_count` | Number of Sybil clusters detected |
| `graph_risk_score` | Overall risk score (0.0-1.0) |
| `graph_is_suspicious` | Binary flag (0/1) for suspicious activity |
| `graph_node_count` | Number of unique wallets |
| `graph_edge_count` | Number of transactions |
| `graph_density` | Graph density (edges/possible_edges) |
| `graph_connected_components` | Number of disconnected components |
| `graph_avg_clustering_coef` | Average clustering coefficient |
| `graph_max_pump_confidence` | Highest pump pattern confidence |
| `graph_max_wash_confidence` | Highest wash trading confidence |
| `graph_avg_cluster_cohesion` | Average wallet cluster cohesion |
| `graph_max_cluster_size` | Largest wallet cluster size |
| `graph_build_time_ms` | Graph construction time |

## Examples

Run the demo:

```bash
cargo run --example demo_graph_analyzer
```

## Testing

Run unit tests:

```bash
# All graph analyzer tests
cargo test --lib graph_analyzer

# Integration tests
cargo test --test test_graph_analyzer

# Performance tests
cargo test --test test_graph_analyzer test_graph_build_performance -- --nocapture
cargo test --test test_graph_analyzer test_memory_usage -- --nocapture
```

## Architecture

```
graph_analyzer/
├── types.rs              # Core data structures
├── graph_builder.rs      # Transaction graph construction
├── pattern_detector.rs   # Pattern detection algorithms
├── clustering.rs         # Wallet clustering
└── mod.rs               # Main analyzer interface
```

## Dependencies

- `petgraph`: Graph data structure and algorithms
- `solana-client`: RPC client for on-chain data
- `chrono`: Timestamp handling

## Future Improvements

- [ ] Historical transaction fetching from RPC
- [ ] Advanced graph algorithms (PageRank, centrality measures)
- [ ] Pattern learning from historical data
- [ ] Multi-token cross-analysis
- [ ] Real-time streaming updates from websockets
- [ ] Integration with DEX liquidity pool analytics
