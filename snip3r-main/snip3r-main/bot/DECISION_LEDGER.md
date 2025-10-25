# DecisionLedger Implementation - I Filar (First Pillar)

This implementation provides the **Operational Memory** for the H-5N1P3R Oracle system, as described in the README as "I Filar: Pamięć Operacyjna – DecisionLedger".

## What Was Implemented

### Core Components

1. **DecisionLedger** (`src/oracle/decision_ledger.rs`)
   - SQLite-based persistent storage for all Oracle decisions
   - Records every decision made by the PredictiveOracle
   - Tracks actual transaction outcomes (profit, loss, failed execution)
   - Provides historical data access for future analysis

2. **TransactionMonitor** (`src/oracle/transaction_monitor.rs`)
   - Monitors the outcome of trading transactions
   - Simulates profit/loss evaluation (50% profit, 30% loss, 20% failed)
   - Updates DecisionLedger with final outcomes
   - Real implementation would integrate with Solana RPC

3. **Enhanced Types** (`src/oracle/types.rs`)
   - `TransactionRecord`: Complete record of decision + outcome
   - `Outcome`: Enum for transaction results (Profit, Loss, Neutral, etc.)
   - Communication channels for async processing

### Database Schema

The system creates a SQLite database (`decisions.db`) with the following structure:

```sql
CREATE TABLE transaction_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    mint TEXT NOT NULL,
    score INTEGER NOT NULL,
    reason TEXT NOT NULL,
    feature_scores TEXT NOT NULL,        -- JSON
    calculation_time INTEGER NOT NULL,
    anomaly_detected BOOLEAN NOT NULL,
    timestamp_decision_made INTEGER NOT NULL,

    transaction_signature TEXT,
    buy_price_sol REAL,
    sell_price_sol REAL,
    amount_bought_tokens REAL,
    amount_sold_tokens REAL,
    initial_sol_spent REAL,
    final_sol_received REAL,

    timestamp_transaction_sent INTEGER,
    timestamp_outcome_evaluated INTEGER,
    actual_outcome TEXT NOT NULL,        -- JSON serialized Outcome
    market_context_snapshot TEXT NOT NULL -- JSON
);
```

## How It Works

1. **Decision Recording**: When the Oracle evaluates a token candidate, it creates a `TransactionRecord` and sends it to the DecisionLedger
2. **Transaction Monitoring**: If a transaction is sent, the TransactionMonitor tracks its outcome
3. **Outcome Updates**: Final profit/loss results are recorded back to the DecisionLedger
4. **Persistent Memory**: All decisions and outcomes are stored in SQLite for future analysis

## Running the Demo

```bash
cargo run
```

This will:
- Create 3 demo token decisions
- Simulate transactions for high-scoring tokens (≥75 score)
- Monitor and record outcomes
- Create a `decisions.db` file with the persistent memory

## Viewing the Data

```bash
# Count total records
sqlite3 decisions.db "SELECT COUNT(*) FROM transaction_records;"

# View decisions and outcomes
sqlite3 decisions.db "SELECT mint, score, actual_outcome FROM transaction_records;"

# View transaction details
sqlite3 decisions.db "SELECT mint, initial_sol_spent, final_sol_received, actual_outcome FROM transaction_records WHERE transaction_signature IS NOT NULL;"
```

## Testing

```bash
cargo test
```

## Future Integration Points

This DecisionLedger provides the foundation for:

1. **II Filar (PerformanceMonitor)**: Will analyze historical records to compute win rate, profit factors, etc.
2. **III Filar (MarketRegimeDetector)**: Will use market context snapshots for regime-aware analysis
3. **Self-Learning Weights**: Will optimize based on which features correlate with profitable outcomes

## Key Benefits

- **Complete Audit Trail**: Every Oracle decision is permanently recorded
- **Learning Foundation**: Historical data enables machine learning and optimization
- **Performance Analysis**: Track actual vs predicted performance over time
- **Debugging**: Full context for understanding why decisions were made
- **Regulatory Compliance**: Complete record of all trading decisions and outcomes

This implementation represents the first critical step toward building a "genius" Oracle that learns from its own decisions and continuously improves its performance.