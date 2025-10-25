# Pillar II Implementation: PerformanceMonitor and StrategyOptimizer

This document describes the implementation of Pillar II - the Self-Control and Evaluation systems that create a closed OODA loop (Observe, Orient, Decide, Act) for autonomous Oracle optimization.

## Overview

Pillar II builds upon Pillar I (DecisionLedger) to create a feedback system that continuously improves Oracle performance by:

1. **Observing** trading performance through the PerformanceMonitor
2. **Orienting** by analyzing which decisions led to losses
3. **Deciding** on parameter adjustments through correlation analysis
4. **Acting** by updating Oracle weights and thresholds

## Architecture

```
┌─────────────────┐    PerformanceReport    ┌─────────────────┐
│ PerformanceMonitor │ ──────────────────────→ │ StrategyOptimizer │
│                   │                       │                 │
│ • Analyzes DB     │                       │ • Finds patterns │
│ • Computes KPIs   │                       │ • Optimizes params│
│ • Sends reports   │                       │ • Sends updates  │
└─────────────────┘                         └─────────────────┘
         ↑                                           │
         │                                           ↓
         │              OptimizedParameters          │
    ┌─────────┐                                 ┌─────────┐
    │   DB    │                                 │  Main   │
    │(Pillar I)│                                │  Loop   │
    └─────────┘                                 └─────────┘
```

## Components

### 1. PerformanceMonitor (`src/oracle/performance_monitor.rs`)

**Responsibility**: Observe and measure Oracle performance

- **Input**: Historical data from DecisionLedger database
- **Processing**: Computes Key Performance Indicators (KPIs)
- **Output**: PerformanceReport with metrics

**Key Metrics**:
- Win Rate: Percentage of profitable trades
- Profit Factor: Ratio of profits to losses
- Average Profit/Loss: Mean values per trade
- Net Profit: Total profit minus total losses

### 2. StrategyOptimizer (`src/oracle/strategy_optimizer.rs`)

**Responsibility**: Orient, Decide, and Act based on performance

- **Input**: PerformanceReport from PerformanceMonitor
- **Processing**: Analyzes losing trades for feature correlations
- **Output**: OptimizedParameters with new weights/thresholds

**Optimization Logic**:
1. Triggers when Profit Factor < 1.2 and sufficient trade data exists
2. Queries database for losing trades
3. Finds features with lowest average scores in losses
4. Increases weight of worst-performing feature by 10%

### 3. Types and Communication (`src/oracle/types.rs`)

**New Types Added**:
- `PerformanceReport`: Contains all performance metrics
- `OptimizedParameters`: New weights, thresholds, and reasoning
- `FeatureWeights`: Weights for all Oracle features
- `ScoreThresholds`: Threshold values for decision making

**Communication Channels**:
- `PerformanceReportSender/Receiver`: Monitor → Optimizer
- `OptimizedParametersSender/Receiver`: Optimizer → Main Loop

## Usage

### Running the Complete System

```bash
# Run the main OODA loop demo
cargo run

# Run optimization test with poor performance data
cargo run --bin test_optimization
```

### Example Output

**Normal Performance** (no optimization needed):
```
PerformanceMonitor: Analysis complete. Profit Factor: 2.38, Win Rate: 66.67%
StrategyOptimizer: Current strategy performance is acceptable (PF: 2.38). No optimization needed.
```

**Poor Performance** (triggers optimization):
```
PerformanceMonitor: Analysis complete. Profit Factor: 0.27, Win Rate: 42.9%
StrategyOptimizer: Profit Factor is below threshold (0.27). Attempting to optimize strategy.
StrategyOptimizer: Found new optimized parameters: Losses are correlated with low scores in 'liquidity' (avg: 0.48). Increasing its weight.
Main Loop: Strategy parameters updated! Oracle would be reconfigured here.
```

## Integration with Existing System

The implementation builds on the existing DecisionLedger (Pillar I) by:

1. **Reusing Database**: Both monitors query the same SQLite database
2. **Minimal Changes**: No modifications to existing DecisionLedger logic
3. **Additive Architecture**: New components run as separate async tasks
4. **Compatible Types**: Uses existing Outcome and TransactionRecord structures

## Configuration

The system uses sensible defaults:
- **PerformanceMonitor**: Analyzes every 15 minutes, 24-hour lookback window
- **StrategyOptimizer**: Optimization threshold of 1.2 profit factor
- **Weight Adjustments**: 10% increase for poorly-performing features

## Future Enhancements

The current implementation provides the foundation for more sophisticated features:

1. **Threshold Optimization**: Adjust ScoreThresholds based on performance
2. **Market Regime Detection**: Different strategies for different market conditions
3. **A/B Testing**: Test multiple parameter sets simultaneously
4. **Machine Learning**: Replace simple heuristics with ML models
5. **Dynamic Reconfiguration**: Hot-reload Oracle parameters without restart

## Testing

The implementation includes comprehensive testing:

- `cargo test`: Runs all unit and integration tests
- `cargo run --bin test_optimization`: Demonstrates optimization path
- Manual verification through logging and database inspection

This Pillar II implementation successfully creates the autonomous feedback loop necessary for a self-improving Oracle system.