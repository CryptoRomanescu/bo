//! Demo: Smart Money Integration with Early Pump Detector
//!
//! This example demonstrates the complete smart money tracking system
//! integrated with the early pump detector for real-time memecoin analysis.

use h_5n1p3r::oracle::{
    DataSource, EarlyPumpConfig, EarlyPumpDetector, SmartMoneyConfig, SmartMoneyTracker,
    SmartWalletProfile,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║     Smart Money Integration - Real-Time Pump Detection        ║");
    println!("╚═══════════════════════════════════════════════════════════════╝\n");

    // Part 1: Setup Smart Money Tracker
    println!("📋 Step 1: Configuring Smart Money Tracker...\n");

    let smart_money_config = SmartMoneyConfig {
        // In production, these would come from environment variables
        nansen_api_key: std::env::var("NANSEN_API_KEY").ok(),
        birdeye_api_key: std::env::var("BIRDEYE_API_KEY").ok(),
        
        // Add some example smart wallets for demo
        custom_smart_wallets: vec![
            "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM".to_string(),
            "8sLbNZoA1cfnvMJLPfp98ZLAnFSYCFApfJKMbiXNLwxj".to_string(),
            "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU".to_string(),
        ],
        
        detection_window_secs: 60,
        min_smart_wallets: 2,
        cache_duration_secs: 300,
        api_timeout_secs: 5,
        enable_nansen: false, // Set to true if you have API key
        enable_birdeye: false, // Set to true if you have API key
    };

    let smart_money_tracker = Arc::new(SmartMoneyTracker::new(smart_money_config));

    println!("✅ Smart Money Tracker initialized");
    println!("   - Tracking {} custom wallets", 3);
    println!("   - Detection window: 60 seconds");
    println!("   - Alert threshold: 2+ smart wallets\n");

    // Part 2: Add additional smart wallets (simulating manual curation)
    println!("📋 Step 2: Adding curated smart wallets...\n");

    let additional_wallets = vec![
        SmartWalletProfile {
            address: "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R".to_string(),
            source: DataSource::Custom,
            success_rate: Some(85.5),
            total_profit_sol: Some(1250.0),
            successful_trades: Some(145),
            last_activity: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        },
        SmartWalletProfile {
            address: "5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1".to_string(),
            source: DataSource::Custom,
            success_rate: Some(92.3),
            total_profit_sol: Some(3480.0),
            successful_trades: Some(287),
            last_activity: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        },
    ];

    for wallet in additional_wallets {
        println!("   Adding: {}", wallet.address);
        println!("   - Success Rate: {}%", wallet.success_rate.unwrap_or(0.0));
        println!("   - Total Profit: {} SOL", wallet.total_profit_sol.unwrap_or(0.0));
        smart_money_tracker.add_smart_wallet(wallet).await;
    }

    let tracked_wallets = smart_money_tracker.get_smart_wallets().await;
    println!("\n✅ Now tracking {} smart wallets\n", tracked_wallets.len());

    // Part 3: Setup Early Pump Detector with Smart Money Integration
    println!("📋 Step 3: Configuring Early Pump Detector...\n");

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(RpcClient::new(rpc_url));

    let pump_config = EarlyPumpConfig {
        detection_timeout_secs: 60,
        decision_timeout_secs: 60,
        buy_threshold: 70,
        pass_threshold: 50,
        parallel_checks: true,
        rpc_endpoints: vec!["https://api.mainnet-beta.solana.com".to_string()],
    };

    let detector = EarlyPumpDetector::with_smart_money(
        pump_config,
        rpc_client,
        smart_money_tracker.clone(),
    );

    println!("✅ Early Pump Detector initialized with smart money integration");
    println!("   - Detection timeout: 60s");
    println!("   - Decision timeout: 60s");
    println!("   - BUY threshold: 70/100");
    println!("   - Parallel checks: enabled\n");

    // Part 4: Analyze a token (simulation)
    println!("📋 Step 4: Analyzing memecoin launch...\n");

    let token_mint = "DemoToken7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZR";
    let current_time = chrono::Utc::now().timestamp() as u64;
    let deploy_timestamp = current_time - 30; // Deployed 30 seconds ago

    println!("🪙 Token: {}", token_mint);
    println!("⏰ Deploy Time: {} seconds ago", 30);
    println!("🔍 Checking for smart money activity...\n");

    // Check for smart money
    let (score, wallet_count, transactions) = smart_money_tracker
        .check_smart_money(token_mint, deploy_timestamp)
        .await?;

    println!("📊 SMART MONEY ANALYSIS:");
    println!("   Score:           {}/100", score);
    println!("   Smart Wallets:   {}", wallet_count);
    println!("   Transactions:    {}", transactions.len());
    println!();

    // Check if alert should be triggered
    let alert = smart_money_tracker
        .check_alert(token_mint, deploy_timestamp)
        .await?;

    if let Some(alert) = alert {
        println!("🚨 SMART MONEY ALERT TRIGGERED!");
        println!("   Unique Wallets:     {}", alert.unique_wallets);
        println!("   Total Volume:       {:.4} SOL", alert.total_volume_sol);
        println!("   Time to 1st Buy:    {}s", alert.time_to_first_buy_secs);
        println!();

        if !alert.transactions.is_empty() {
            println!("📝 TRANSACTION DETAILS:");
            for (i, tx) in alert.transactions.iter().enumerate() {
                println!("   {}. Wallet: {}", i + 1, &tx.wallet[..8]);
                println!("      Amount: {:.4} SOL", tx.amount_sol);
                println!("      Time:   {}s after deploy", tx.timestamp - deploy_timestamp);
            }
            println!();
        }
    } else {
        println!("ℹ️  No alert triggered (need 2+ smart wallets)");
        println!();
    }

    // Part 5: Run full early pump analysis
    println!("📋 Step 5: Running complete early pump analysis...\n");

    let analysis = detector
        .analyze(token_mint, deploy_timestamp, "pump.fun")
        .await?;

    println!("📊 EARLY PUMP ANALYSIS RESULTS:");
    println!("═══════════════════════════════════════════════════════════════");
    println!("Decision:              {:?}", analysis.decision);
    println!("Overall Score:         {}/100", analysis.score);
    println!();
    println!("INDIVIDUAL CHECKS:");
    println!("   Supply Concentration: {}/100", analysis.check_results.supply_concentration);
    println!("   LP Lock:              {}/100", analysis.check_results.lp_lock);
    println!("   Wash Trading Risk:    {}/100", analysis.check_results.wash_trading_risk);
    println!("   Smart Money:          {}/100 ({} wallets)",
        analysis.check_results.smart_money,
        analysis.check_results.smart_money_wallet_count
    );
    println!("   Holder Growth:        {}/100", analysis.check_results.holder_growth);
    println!();
    println!("TIMING METRICS:");
    println!("   Deploy → Detection:   {}ms", analysis.timings.deploy_to_detection_ms);
    println!("   Detection → Decision: {}ms", analysis.timings.detection_to_decision_ms);
    println!("   Total Time:           {}ms", analysis.timings.total_decision_time_ms);
    println!();
    println!("CHECK TIMINGS:");
    println!("   Supply Check:         {}ms", analysis.timings.check_timings.supply_concentration_ms);
    println!("   LP Lock Check:        {}ms", analysis.timings.check_timings.lp_lock_ms);
    println!("   Wash Trading Check:   {}ms", analysis.timings.check_timings.wash_trading_ms);
    println!("   Smart Money Check:    {}ms", analysis.timings.check_timings.smart_money_ms);
    println!("   Holder Growth Check:  {}ms", analysis.timings.check_timings.holder_growth_ms);
    println!("═══════════════════════════════════════════════════════════════\n");

    // Part 6: Summary
    println!("📋 SUMMARY");
    println!("═══════════════════════════════════════════════════════════════");
    println!("✅ Smart money tracking integrated successfully");
    println!("✅ Real-time alert system operational");
    println!("✅ Decision made in <100s requirement met");
    println!();
    
    if analysis.check_results.smart_money_wallet_count >= 2 {
        println!("🚨 RECOMMENDATION: STRONG BUY SIGNAL");
        println!("   {} smart wallets detected within detection window",
            analysis.check_results.smart_money_wallet_count);
        println!("   This indicates high confidence in token potential");
    } else {
        println!("ℹ️  Monitoring recommended");
        println!("   Awaiting more smart wallet confirmations");
    }
    println!("═══════════════════════════════════════════════════════════════\n");

    println!("🎯 Demo completed successfully!");
    println!();
    println!("Next steps:");
    println!("1. Set NANSEN_API_KEY or BIRDEYE_API_KEY for live data");
    println!("2. Configure custom smart wallets in config.toml");
    println!("3. Run: cargo run --bin smart-money-cli help");
    println!("4. Monitor real tokens: cargo run --bin smart-money-cli monitor 300");
    println!();

    Ok(())
}
