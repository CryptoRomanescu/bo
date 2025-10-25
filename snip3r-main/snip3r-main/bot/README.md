# H-5N1P3R Trading Bot

A high-performance Solana trading oracle system with operational memory capabilities.

## Features

### Core Components

- **PredictiveOracle**: Universe-class predictive oracle for token analysis
- **DecisionLedger**: Persistent operational memory for decisions and outcomes
- **TransactionMonitor**: On-chain transaction tracking and verification
- **PerformanceMonitor**: Real-time performance analysis
- **StrategyOptimizer**: Adaptive parameter optimization
- **MarketRegimeDetector**: Market condition analysis

### Advanced Capabilities

- **Bot Activity & Bundle Detector**: Universe-class bot activity and coordinated bundle detection for memecoin launches with <10s latency (see [BOT_BUNDLE_DETECTOR.md](BOT_BUNDLE_DETECTOR.md))
- **Early Pump Detector**: Ultra-fast <100s decision engine for memecoin launches (see [EARLY_PUMP_DETECTOR.md](EARLY_PUMP_DETECTOR.md))
- **Supply Concentration Analyzer**: Universe-class token holder distribution analysis with Gini coefficient and auto-reject for >70% concentration (see [SUPPLY_CONCENTRATION_ANALYZER.md](SUPPLY_CONCENTRATION_ANALYZER.md))
- **LP Lock Verifier**: Universe-class LP token lock and burn detection for rug-pull protection (see [LP_LOCK_VERIFIER.md](LP_LOCK_VERIFIER.md))
- **Wash Trading Detector**: Graph-based wash trading and fake volume detection (see [WASH_TRADING_DETECTOR.md](WASH_TRADING_DETECTOR.md))
- **Smart Money Integration**: Real-time monitoring of smart wallet activity with Nansen/Birdeye (see [SMART_MONEY_INTEGRATION.md](SMART_MONEY_INTEGRATION.md))
- **Metacognition**: Confidence calibration and self-awareness
- **Pattern Memory**: Temporal pattern recognition and learning
- **Actor System**: Fault-tolerant, scalable architecture (see [ACTOR_SYSTEM.md](ACTOR_SYSTEM.md))
- **Feature Engineering**: ML feature engineering framework with 200+ features (see [FEATURE_ENGINEERING.md](FEATURE_ENGINEERING.md))
- **Quantum Ensemble**: Multi-strategy oracle system with Bayesian voting (see [ENSEMBLE_ARCHITECTURE.md](ENSEMBLE_ARCHITECTURE.md))
- **Graph Analyzer**: On-chain transaction graph analysis for pump/wash detection (see [GRAPH_ANALYZER.md](GRAPH_ANALYZER.md))

### Security Features

- **Authentication**: Bearer token authentication for metrics endpoint
- **Rate Limiting**: IP-based rate limiting to prevent abuse
- **Input Validation**: Comprehensive validation for all external inputs
- **Secure Configuration**: Environment variable support for secrets
- **Constant-Time Comparison**: Timing attack prevention for authentication
- **Security Documentation**: See [SECURITY.md](SECURITY.md) for complete security posture

## Architecture

The system is built on an actor-based architecture using [actix](https://actix.rs/):

```
┌─────────────────────────────────────────────┐
│          SupervisorActor                     │
│  - Manages actor lifecycle                   │
│  - Implements restart strategies             │
└──────────┬──────────┬──────────┬─────────────┘
           │          │          │
    ┌──────▼──┐  ┌───▼─────┐  ┌─▼────────┐
    │ Oracle  │  │ Storage │  │ Monitor  │
    │ Actor   │  │ Actor   │  │ Actor    │
    └─────────┘  └─────────┘  └──────────┘
```

See [ACTOR_SYSTEM.md](ACTOR_SYSTEM.md) for detailed documentation.

## Getting Started

### Prerequisites

- Rust 1.70+
- SQLite 3.x

### Installation

```bash
git clone https://github.com/Korbol997/snip3r.git
cd snip3r/bot
cargo build --release
```

### Configuration

1. Copy the configuration template:
```bash
cp config.toml.template config.toml
```

2. Set up environment variables for secrets (recommended):
```bash
# Create .env file from template
cp .env.example .env

# Edit .env with your actual values
# IMPORTANT: Never commit .env to version control
nano .env
```

3. Generate a secure authentication token for metrics:
```bash
export METRICS_AUTH_TOKEN=$(openssl rand -base64 32)
```

4. Configure your RPC endpoint:
```bash
# Use environment variable (recommended for production)
export RPC_URL="https://your-private-rpc-endpoint.com"

# Or edit config.toml (not recommended for secrets)
```

See [SECURITY.md](SECURITY.md) for comprehensive security configuration guidelines.

### Running Examples

```bash
# Early pump detector demo (ultra-fast memecoin analysis)
cargo run --example demo_early_pump_detector

# Early pump detector CLI
cargo run --bin early-pump-cli help
cargo run --bin early-pump-cli analyze <mint> <program>
cargo run --bin early-pump-cli batch 100

# Smart money tracker CLI
cargo run --bin smart-money-cli help
cargo run --bin smart-money-cli check <mint>
cargo run --bin smart-money-cli alert <mint>
cargo run --bin smart-money-cli monitor 300

# Actor system demo
cargo run --example demo_actor_system

# Full system demo
cargo run --example demo_full_system

# Metacognition demo
cargo run --example demo_metacognition

# Feature engineering demo
cargo run --example demo_feature_engineering

# Quantum Ensemble system demo
cargo run --example demo_ensemble

# Graph analyzer demo
cargo run --example demo_graph_analyzer

# Wash trading detector demo
cargo run --example demo_wash_trading_detector

# Supply concentration analyzer demo (NEW!)
cargo run --example demo_supply_concentration_analyzer
```

## Testing

```bash
# Run all tests
cargo test

# Run specific test suites
cargo test --test test_security          # Security tests
cargo test --test test_actor_system      # Actor system tests
cargo test --test test_ensemble_integration  # Ensemble tests

# Run with logging
RUST_LOG=info cargo test

# Run security audit
cargo audit
```

## Documentation

- [SUPPLY_CONCENTRATION_ANALYZER.md](SUPPLY_CONCENTRATION_ANALYZER.md) - **Universe-class token holder distribution analysis with Gini coefficient**
- [WASH_TRADING_DETECTOR.md](WASH_TRADING_DETECTOR.md) - **Graph-based wash trading and fake volume detection**
- [LP_LOCK_VERIFIER.md](LP_LOCK_VERIFIER.md) - **Universe-class LP lock and burn verification for rug-pull protection**
- [SMART_MONEY_INTEGRATION.md](SMART_MONEY_INTEGRATION.md) - **Real-time smart money monitoring with Nansen/Birdeye**
- [EARLY_PUMP_DETECTOR.md](EARLY_PUMP_DETECTOR.md) - **Ultra-fast <100s decision engine for memecoin launches**
- [SECURITY.md](SECURITY.md) - **Security policy, authentication, and best practices**
- [ACTOR_SYSTEM.md](ACTOR_SYSTEM.md) - Actor system architecture and usage
- [DECISION_LEDGER.md](DECISION_LEDGER.md) - DecisionLedger operational memory
- [METACOGNITION.md](METACOGNITION.md) - Metacognitive awareness system
- [PATTERN_MEMORY.md](PATTERN_MEMORY.md) - Temporal pattern recognition
- [FEATURE_ENGINEERING.md](FEATURE_ENGINEERING.md) - ML feature engineering framework
- [ENSEMBLE_ARCHITECTURE.md](ENSEMBLE_ARCHITECTURE.md) - Quantum Ensemble Oracle System
- [GRAPH_ANALYZER.md](GRAPH_ANALYZER.md) - On-chain graph analysis for pattern detection

## Features Roadmap

- [x] Actor System Foundation
- [x] PredictiveOracle with Metacognition
- [x] DecisionLedger operational memory
- [x] TransactionMonitor with on-chain verification
- [x] Pattern Memory for temporal learning
- [x] ML Feature Engineering Framework (200+ features)
- [x] Quantum Ensemble Oracle System
- [x] **Security Hardening (Authentication, Rate Limiting, Input Validation)**
- [x] **Comprehensive Security Documentation**
- [x] **Early Pump Detector (<100s Decision Engine)**
- [x] **Smart Money Integration (Nansen/Birdeye Real-Time Tracking)**
- [x] **LP Lock Verifier (Universe-Class Rug-Pull Protection)**
- [x] **Wash Trading Detector (Graph-Based Fake Volume Detection)**
- [x] **Supply Concentration Analyzer (Token Holder Distribution with Gini Coefficient)**
- [ ] Remote Actor Distribution
- [ ] Prometheus Metrics Integration
- [ ] Dynamic Actor Scaling

## License

[Add your license here]
