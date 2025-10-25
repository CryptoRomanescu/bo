# Smart Money Integration - Implementation Guide

## Overview

The Smart Money Integration provides real-time monitoring of smart wallet activity on Solana to detect early pump opportunities. This system tracks purchases by high-value wallets and generates alerts when multiple smart wallets buy a token within 60 seconds of deployment.

## Architecture

### Core Components

1. **SmartMoneyTracker** (`src/oracle/smart_money_tracker.rs`)
   - Manages smart wallet profiles and transactions
   - Integrates with Nansen and Birdeye APIs
   - Implements caching and performance optimization
   - Calculates smart money scores (0-100)

2. **EarlyPumpDetector Integration** (`src/oracle/early_pump_detector.rs`)
   - Enhanced with real-time smart money detection
   - Parallel execution for <5s response time
   - Alert generation when >2 smart wallets buy within 60s
   - Comprehensive logging of wallet activity

3. **Smart Money CLI** (`src/bin/smart_money_cli.rs`)
   - Command-line interface for monitoring and analysis
   - Real-time alerts and notifications
   - Wallet management and tracking

## Features

### Key Capabilities

- ✅ **Multi-Source Integration**: Nansen, Birdeye, and custom wallet lists
- ✅ **Real-Time Detection**: <5s latency from transaction to alert
- ✅ **Alert System**: Automatic BUY signals when >2 smart wallets detected
- ✅ **Comprehensive Logging**: wallet address, amount, time, token, transaction signature
- ✅ **Performance Optimized**: Processes 100 tokens in <10s
- ✅ **Detection Window**: 60-second window from token deployment
- ✅ **Caching**: 300-second API response caching to reduce load

### Smart Money Score Calculation

The system calculates a 0-100 score based on:

1. **Number of Unique Wallets** (40 points max)
   - 10 points per smart wallet
   - More wallets = higher confidence

2. **Total Buy Volume** (30 points max)
   - Based on SOL amount purchased
   - Higher volume = stronger signal

3. **Speed to Market** (30 points max)
   - Time from deployment to first buy
   - <10s = 30 points
   - <30s = 20 points
   - <60s = 10 points

## Configuration

### Environment Variables

Add to `.env` file:

```bash
# Smart Money Tracking API Keys
NANSEN_API_KEY=your_nansen_api_key_here
BIRDEYE_API_KEY=your_birdeye_api_key_here

# Smart Money Configuration (optional)
SMART_MONEY_DETECTION_WINDOW_SECS=60
SMART_MONEY_MIN_WALLETS=2
SMART_MONEY_ENABLE_NANSEN=false
SMART_MONEY_ENABLE_BIRDEYE=true
```

### config.toml Settings

Add to `config.toml`:

```toml
[smart_money]
# Detection window for smart wallet purchases (seconds)
detection_window_secs = 60
# Minimum number of smart wallets to trigger alert
min_smart_wallets = 2
# API cache duration (seconds)
cache_duration_secs = 300
# API request timeout (seconds)
api_timeout_secs = 5
# Enable Nansen integration (requires API key)
enable_nansen = false
# Enable Birdeye integration (requires API key)
enable_birdeye = true
```

## Usage

### CLI Commands

#### Check Smart Money for Token

```bash
cargo run --bin smart-money-cli check <token_mint> [deploy_timestamp]
```

Example:
```bash
cargo run --bin smart-money-cli check 7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU
```

Output:
```
🔍 Checking smart money for token: 7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU
Deploy timestamp: 1704067200
Detection window: 60 seconds

✅ Check completed in 0.45s

📊 RESULTS:
  Smart Money Score:  85/100
  Unique Wallets:     3
  Transactions:       3

📝 TRANSACTION DETAILS:
  1. Wallet: 9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM
     Amount: 10.5000 SOL
     Time:   1704067205
     Tx:     5j7s8k9...
     Source: Nansen

🚨 RECOMMENDATION: STRONG BUY SIGNAL
   3 smart wallets detected within 60s window
```

#### Check Alert Status

```bash
cargo run --bin smart-money-cli alert <token_mint> [deploy_timestamp]
```

Example:
```bash
cargo run --bin smart-money-cli alert 7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU
```

Output:
```
🚨 SMART MONEY ALERT TRIGGERED!
   Response time: 0.48s

📊 ALERT DETAILS:
   Token:           7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU
   Unique Wallets:  3
   Total Volume:    125.7500 SOL
   Time to 1st Buy: 5s
   Transactions:    3

✅ ACTION: EXECUTE BUY ORDER
   Confidence: HIGH
   Reason: 3 smart wallets within 5s
```

#### Add Custom Smart Wallet

```bash
cargo run --bin smart-money-cli add-wallet <wallet_address>
```

Example:
```bash
cargo run --bin smart-money-cli add-wallet 9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM
```

#### List Tracked Wallets

```bash
cargo run --bin smart-money-cli list-wallets
```

#### Monitor Smart Money Activity

```bash
cargo run --bin smart-money-cli monitor [duration_secs]
```

Example:
```bash
cargo run --bin smart-money-cli monitor 300  # Monitor for 5 minutes
```

### Programmatic Usage

#### Basic Integration

```rust
use h_5n1p3r::oracle::{SmartMoneyConfig, SmartMoneyTracker};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // Configure smart money tracker
    let config = SmartMoneyConfig {
        birdeye_api_key: Some("your_api_key".to_string()),
        enable_birdeye: true,
        detection_window_secs: 60,
        min_smart_wallets: 2,
        ..Default::default()
    };
    
    let tracker = Arc::new(SmartMoneyTracker::new(config));
    
    // Check smart money for a token
    let token_mint = "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU";
    let deploy_timestamp = chrono::Utc::now().timestamp() as u64 - 30;
    
    let (score, wallet_count, transactions) = tracker
        .check_smart_money(token_mint, deploy_timestamp)
        .await
        .unwrap();
    
    println!("Score: {}, Wallets: {}", score, wallet_count);
    
    // Check for alert
    if let Some(alert) = tracker.check_alert(token_mint, deploy_timestamp).await.unwrap() {
        println!("🚨 ALERT: {} smart wallets!", alert.unique_wallets);
        println!("Total volume: {} SOL", alert.total_volume_sol);
    }
}
```

#### Integration with Early Pump Detector

```rust
use h_5n1p3r::oracle::{EarlyPumpConfig, EarlyPumpDetector, SmartMoneyConfig, SmartMoneyTracker};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // Setup RPC client
    let rpc_client = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
    
    // Setup smart money tracker
    let smart_money_config = SmartMoneyConfig {
        birdeye_api_key: Some("your_api_key".to_string()),
        enable_birdeye: true,
        ..Default::default()
    };
    let smart_money_tracker = Arc::new(SmartMoneyTracker::new(smart_money_config));
    
    // Create early pump detector with smart money integration
    let config = EarlyPumpConfig::default();
    let detector = EarlyPumpDetector::with_smart_money(
        config,
        rpc_client,
        smart_money_tracker,
    );
    
    // Analyze a token
    let token_mint = "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU";
    let deploy_timestamp = chrono::Utc::now().timestamp() as u64 - 30;
    
    let analysis = detector
        .analyze(token_mint, deploy_timestamp, "pump.fun")
        .await
        .unwrap();
    
    println!("Decision: {:?}", analysis.decision);
    println!("Smart wallets: {}", analysis.check_results.smart_money_wallet_count);
}
```

## API Integration Details

### Nansen API

The system supports Nansen's smart money API for tracking wallet activity:

- **Endpoint**: `https://api.nansen.ai/v1/token/{mint}/smart-money`
- **Authentication**: X-API-KEY header
- **Response**: List of smart wallet transactions
- **Rate Limits**: Cached for 300 seconds to minimize API calls

### Birdeye API

Fallback integration with Birdeye for wallet tracking:

- **Endpoint**: `https://public-api.birdeye.so/defi/txs/token?address={mint}`
- **Authentication**: X-API-KEY header
- **Chain**: Solana (x-chain header)
- **Response**: Token transaction history
- **Filtering**: Transactions filtered against known smart wallet list

## Performance Characteristics

### Latency Targets

- ✅ **Alert Generation**: <5s from transaction to alert
- ✅ **Single Token Check**: ~450ms average
- ✅ **Batch Processing**: 100 tokens in <10s
- ✅ **Detection Window**: 60s from token deployment

### Resource Usage

- **Memory**: ~50MB for 1000 tracked wallets
- **Cache**: Up to 10,000 cached API responses (auto-eviction)
- **Network**: 1-5 API calls per token check (with caching)

## Testing

### Run All Tests

```bash
cargo test --test test_smart_money_tracker
```

### Run Specific Tests

```bash
# Test basic functionality
cargo test test_smart_money_tracker_creation

# Test alert system
cargo test test_alert_generation_timing

# Test performance
cargo test test_performance_100_tokens
```

### Test Coverage

- ✅ 11 unit/integration tests
- ✅ Configuration validation
- ✅ Wallet management
- ✅ Score calculation
- ✅ Alert threshold enforcement
- ✅ Performance benchmarks
- ✅ Concurrent access

## Security Considerations

### API Key Management

- **Never commit** API keys to version control
- Use environment variables for all sensitive data
- Rotate keys regularly
- Monitor API usage for anomalies

### Rate Limiting

- Implement exponential backoff for API failures
- Cache responses to minimize API calls
- Monitor rate limit headers from providers

### Data Validation

- Validate all wallet addresses
- Sanitize token mint addresses
- Verify transaction signatures
- Check timestamp ranges

## Troubleshooting

### No API Keys Configured

```
⚠️  Warning: No API keys configured
   Set NANSEN_API_KEY or BIRDEYE_API_KEY environment variables
   Smart money tracking will use fallback mode
```

**Solution**: Set at least one API key in your `.env` file.

### Alert Not Triggering

If alerts aren't triggering despite smart wallet activity:

1. Check `min_smart_wallets` configuration (default: 2)
2. Verify `detection_window_secs` is appropriate (default: 60)
3. Ensure wallets are in the tracking list
4. Check API keys are valid and not rate-limited

### Slow Performance

If checks are taking >5 seconds:

1. Verify network connectivity
2. Check API rate limits
3. Enable caching if disabled
4. Consider using custom wallet list for faster lookups

## Roadmap

### Planned Enhancements

- [ ] Webhook support for external integrations
- [ ] Database persistence for historical analysis
- [ ] Machine learning for wallet classification
- [ ] Additional data source integrations (Galaxy Digital, etc.)
- [ ] Real-time WebSocket connections for instant updates
- [ ] Advanced pattern recognition for pump detection
- [ ] Multi-chain support (Ethereum, BSC, etc.)

## References

- [CryptoLemur 2025 H2 Analysis](../analiza2.txt)
- [Nansen API Documentation](https://docs.nansen.ai/)
- [Birdeye API Documentation](https://docs.birdeye.so/)
- [Early Pump Detector](./EARLY_PUMP_DETECTOR.md)
- [100-Second Decision Engine](./IMPLEMENTATION_NOTES_EARLY_PUMP.md)

## Support

For issues or questions:

1. Check existing tests for usage examples
2. Review CLI help: `cargo run --bin smart-money-cli help`
3. Enable debug logging: `RUST_LOG=debug cargo run --bin smart-money-cli`
4. Check API provider status and rate limits

## License

See repository LICENSE file.
