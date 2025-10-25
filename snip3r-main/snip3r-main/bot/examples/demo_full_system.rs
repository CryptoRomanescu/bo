//! Full system demonstration of H-5N1P3R Oracle System
//!
//! This example demonstrates all three pillars (DecisionLedger, OODA Loop, and MarketRegimeDetector)
//! working together with real-time hot-swapping capabilities.

use anyhow::Result;
use h_5n1p3r::oracle::{
    DecisionLedger, DecisionRecordSender, FeatureWeights, MarketRegime, MarketRegimeDetector,
    MonitoredTransaction, OracleConfig, OracleDataSources, Outcome, PerformanceMonitor,
    PredictiveOracle, ScoreThresholds, ScoredCandidate, StrategyOptimizer, TransactionMonitor,
    TransactionRecord,
};
use h_5n1p3r::types::PremintCandidate;
use h_5n1p3r::{ChannelFactory, OrchestratorConfig, ShutdownCoordinator};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info, Level};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    info!("Starting H-5N1P3R Full System Demo");

    // Load or use default configuration
    let config = OrchestratorConfig::default();

    // Create channel factory
    let channel_factory = ChannelFactory::new(config.channel_buffer_size);

    // Create all communication channels
    let mut ledger_channels = channel_factory.create_decision_ledger_channels();
    let mut ooda_channels = channel_factory.create_ooda_channels();
    let oracle_channels = channel_factory.create_oracle_channels();

    // Initialize DecisionLedger
    let decision_ledger = DecisionLedger::new(
        ledger_channels.decision_record_receiver,
        ledger_channels.outcome_update_receiver,
    )
    .await?;

    // Get database pool for Pillar II components
    let db_pool = decision_ledger
        .get_db_pool()
        .expect("Expected SQLite storage for backward compatibility")
        .clone();

    // Get storage for TransactionMonitor
    let storage = decision_ledger.get_storage();

    // Initialize TransactionMonitor with RPC client
    let rpc_client = Arc::new(RpcClient::new_with_timeout(
        config.rpc_url.clone(),
        Duration::from_secs(config.rpc_timeout_secs),
    ));

    let transaction_monitor = TransactionMonitor::new(
        storage,
        ledger_channels.outcome_update_sender.clone(),
        config.tx_monitor.check_interval_ms,
        rpc_client,
        config.wallet_pubkey.clone(),
    );

    // Initialize Pillar II components
    let initial_weights = FeatureWeights::default();
    let initial_thresholds = ScoreThresholds::default();

    let performance_monitor = PerformanceMonitor::new(
        db_pool.clone(),
        ooda_channels.perf_report_sender,
        config.perf_monitor.analysis_interval_minutes,
        config.perf_monitor.lookback_hours,
    );

    let strategy_optimizer = StrategyOptimizer::new(
        db_pool,
        ooda_channels.perf_report_receiver,
        ooda_channels.opt_params_sender,
        initial_weights.clone(),
        initial_thresholds.clone(),
    );

    // Initialize PredictiveOracle for Hot-Swap Demonstration
    info!("Initializing PredictiveOracle with hot-swap capability");

    let shared_oracle_config = Arc::new(RwLock::new(
        h_5n1p3r::oracle::quantum_oracle::SimpleOracleConfig::default(),
    ));

    let oracle = Arc::new(PredictiveOracle::new(
        oracle_channels.candidate_receiver,
        oracle_channels.oracle_scored_sender,
        shared_oracle_config.clone(),
    )?);

    info!("PredictiveOracle initialized with hot-swap capability");

    // Initialize Pillar III: MarketRegimeDetector
    info!("Initializing Pillar III: MarketRegimeDetector");

    let current_market_regime = Arc::new(RwLock::new(MarketRegime::LowActivity));
    let oracle_config = OracleConfig::default();

    let http_client = reqwest::Client::new();
    let data_sources_for_regime = Arc::new(OracleDataSources::new(
        vec![], // Empty RPC clients for now (placeholder)
        http_client,
        oracle_config,
    ));

    let regime_detector = MarketRegimeDetector::new(
        data_sources_for_regime,
        current_market_regime.clone(),
        config.regime_detector.analysis_interval_secs,
    );

    info!("MarketRegimeDetector initialized successfully");

    // Initialize shutdown coordinator
    let mut shutdown = ShutdownCoordinator::new();

    // Start all components as background tasks
    shutdown.add_handle(tokio::spawn(async move {
        decision_ledger.run().await;
    }));

    shutdown.add_handle(tokio::spawn(async move {
        transaction_monitor
            .run(ledger_channels.monitor_tx_receiver)
            .await;
    }));

    shutdown.add_handle(tokio::spawn(async move {
        performance_monitor.run().await;
    }));

    shutdown.add_handle(tokio::spawn(async move {
        strategy_optimizer.run().await;
    }));

    shutdown.add_handle(tokio::spawn(async move {
        regime_detector.run().await;
    }));

    // Start Oracle
    let _oracle_clone = oracle.clone();
    shutdown.add_handle(tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
    }));

    // Handle Oracle scoring results
    let mut oracle_scored_receiver = oracle_channels.oracle_scored_receiver;
    shutdown.add_handle(tokio::spawn(async move {
        while let Some(scored) = oracle_scored_receiver.recv().await {
            info!(
                "Oracle scored candidate: {} with score {}",
                scored.mint, scored.predicted_score
            );
        }
    }));

    // Start enhanced OODA loop coordination task
    let current_regime_clone = current_market_regime.clone();
    let oracle_for_hotswap = oracle.clone();
    shutdown.add_handle(tokio::spawn(async move {
        info!("Enhanced OODA Loop coordinator started with Pillar III regime awareness...");

        // Create a periodic task to log current market regime
        let regime_monitor = current_regime_clone.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                let current_regime = *regime_monitor.read().await;
                info!("Current Market Regime: {:?}", current_regime);
            }
        });

        // Handle strategy optimization updates with hot-swap implementation
        while let Some(new_params) = ooda_channels.opt_params_receiver.recv().await {
            let current_regime = *current_regime_clone.read().await;

            info!("OODA Loop: Received new optimized parameters!");
            info!("Current Market Regime: {:?}", current_regime);
            info!("Optimization Reason: {}", new_params.reason);
            info!(
                "New liquidity weight: {:.3}",
                new_params.new_weights.liquidity
            );
            info!(
                "New holder_distribution weight: {:.3}",
                new_params.new_weights.holder_distribution
            );

            // Hot-swap Oracle configuration in real-time
            match oracle_for_hotswap
                .update_config(new_params.new_weights, new_params.new_thresholds)
                .await
            {
                Ok(()) => {
                    info!("✅ OODA Loop ACT Phase: Oracle configuration hot-swapped successfully!");
                    info!("🔄 Oracle is now using the new optimized parameters without restart");
                }
                Err(e) => {
                    error!("❌ Failed to hot-swap Oracle configuration: {}", e);
                }
            }
        }
    }));

    // Demo: Create and record some decisions
    demo_decision_recording(
        ledger_channels.decision_record_sender,
        ledger_channels.monitor_tx_sender,
    )
    .await?;

    // Let the system run to demonstrate the complete cycle
    info!("System running... Demonstrating enhanced OODA loop with Pillar III for 30 seconds");
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

    info!("Demo completed. The complete enhanced OODA loop system has been demonstrated:");
    info!("- Pillar I: DecisionLedger recorded decisions and outcomes");
    info!("- Pillar II: PerformanceMonitor analyzed performance and StrategyOptimizer provided feedback");
    info!("- Pillar III: MarketRegimeDetector provided contextual market awareness");
    info!("- Hot-Swap: PredictiveOracle configuration can be updated in real-time");
    info!("Database file 'decisions.db' contains the persistent memory.");

    // Graceful shutdown
    shutdown.shutdown().await;

    Ok(())
}

/// Demonstrate recording decisions to the DecisionLedger
async fn demo_decision_recording(
    decision_sender: DecisionRecordSender,
    monitor_sender: mpsc::Sender<MonitoredTransaction>,
) -> Result<()> {
    info!("Creating demo PredictiveOracle decisions...");

    // Create some example decisions
    for i in 1..=3 {
        let mint_address = format!(
            "DemoToken{}Address{}",
            i,
            chrono::Utc::now().timestamp_millis()
        );
        let creator_address = format!(
            "DemoCreator{}Address{}",
            i,
            chrono::Utc::now().timestamp_millis()
        );

        let candidate = PremintCandidate {
            mint: mint_address.clone(),
            creator: creator_address,
            program: "pump.fun".to_string(),
            slot: 12345 + i,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            instruction_summary: Some("Token creation".to_string()),
            is_jito_bundle: Some(true),
        };

        let mut feature_scores = HashMap::new();
        feature_scores.insert("liquidity".to_string(), 0.7 + (i as f64 * 0.1));
        feature_scores.insert("holder_distribution".to_string(), 0.6);
        feature_scores.insert("volume_growth".to_string(), 0.8);

        let scored_candidate = ScoredCandidate {
            base: candidate.clone(),
            mint: candidate.mint.clone(),
            predicted_score: 70 + (i * 5) as u8,
            reason: format!("High potential token #{}", i),
            feature_scores: feature_scores.clone(),
            calculation_time: 150_000,
            anomaly_detected: false,
            timestamp: candidate.timestamp,
        };

        // Record the initial decision
        let initial_record = TransactionRecord {
            id: None,
            scored_candidate: scored_candidate.clone(),
            transaction_signature: None,
            buy_price_sol: None,
            sell_price_sol: None,
            amount_bought_tokens: None,
            amount_sold_tokens: None,
            initial_sol_spent: None,
            final_sol_received: None,
            timestamp_decision_made: candidate.timestamp,
            timestamp_transaction_sent: None,
            timestamp_outcome_evaluated: None,
            actual_outcome: Outcome::NotExecuted,
            market_context_snapshot: HashMap::new(),
        };

        decision_sender.send(initial_record).await?;
        info!("Recorded decision for token: {}", candidate.mint);

        // For high-scoring candidates, simulate sending a transaction
        if scored_candidate.predicted_score >= 75 {
            let tx_signature = format!("SimulatedTx{}_{}", i, candidate.timestamp);
            let amount_bought = 1000.0;
            let sol_spent = 1.0;

            // Send transaction to monitor
            let monitored_tx = MonitoredTransaction {
                signature: tx_signature.clone(),
                mint: candidate.mint.clone(),
                amount_bought_tokens: amount_bought,
                initial_sol_spent: sol_spent,
                monitor_until: chrono::Utc::now().timestamp_millis() as u64 + 10_000,
            };

            monitor_sender.send(monitored_tx).await?;

            // Record that transaction was sent
            let sent_record = TransactionRecord {
                id: None,
                scored_candidate: scored_candidate.clone(),
                transaction_signature: Some(tx_signature.clone()),
                buy_price_sol: Some(sol_spent / amount_bought),
                sell_price_sol: None,
                amount_bought_tokens: Some(amount_bought),
                amount_sold_tokens: None,
                initial_sol_spent: Some(sol_spent),
                final_sol_received: None,
                timestamp_decision_made: candidate.timestamp,
                timestamp_transaction_sent: Some(chrono::Utc::now().timestamp_millis() as u64),
                timestamp_outcome_evaluated: None,
                actual_outcome: Outcome::PendingConfirmation,
                market_context_snapshot: HashMap::new(),
            };

            decision_sender.send(sent_record).await?;
            info!(
                "Sent transaction for token: {} (signature: {})",
                candidate.mint, tx_signature
            );
        }

        // Small delay between decisions
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    info!("All demo decisions recorded!");
    Ok(())
}
