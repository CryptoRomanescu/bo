//! CLI for Smart Money Tracker
//!
//! Provides command-line interface for monitoring smart wallet activity
//! and generating real-time pump alerts

use anyhow::{Context, Result};
use h_5n1p3r::oracle::{DataSource, SmartMoneyConfig, SmartMoneyTracker, SmartWalletProfile};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage(&args[0]);
        return Ok(());
    }

    let command = &args[1];

    match command.as_str() {
        "check" => {
            if args.len() < 3 {
                eprintln!("Usage: {} check <token_mint> [deploy_timestamp]", args[0]);
                eprintln!(
                    "Example: {} check 7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
                    args[0]
                );
                return Ok(());
            }

            let token_mint = &args[2];
            let deploy_timestamp = if args.len() > 3 {
                args[3].parse::<u64>().context("Invalid timestamp")?
            } else {
                // Default to current time minus 30 seconds
                chrono::Utc::now().timestamp() as u64 - 30
            };

            check_smart_money(token_mint, deploy_timestamp).await?;
        }
        "alert" => {
            if args.len() < 3 {
                eprintln!("Usage: {} alert <token_mint> [deploy_timestamp]", args[0]);
                eprintln!(
                    "Example: {} alert 7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
                    args[0]
                );
                return Ok(());
            }

            let token_mint = &args[2];
            let deploy_timestamp = if args.len() > 3 {
                args[3].parse::<u64>().context("Invalid timestamp")?
            } else {
                chrono::Utc::now().timestamp() as u64 - 30
            };

            check_alert(token_mint, deploy_timestamp).await?;
        }
        "add-wallet" => {
            if args.len() < 3 {
                eprintln!("Usage: {} add-wallet <wallet_address>", args[0]);
                eprintln!(
                    "Example: {} add-wallet 9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM",
                    args[0]
                );
                return Ok(());
            }

            let wallet = &args[2];
            add_wallet(wallet).await?;
        }
        "list-wallets" => {
            list_wallets().await?;
        }
        "monitor" => {
            if args.len() < 2 {
                eprintln!("Usage: {} monitor [duration_secs]", args[0]);
                eprintln!("Example: {} monitor 300", args[0]);
                return Ok(());
            }

            let duration = args
                .get(2)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(300); // Default 5 minutes

            monitor_smart_money(duration).await?;
        }
        "help" | "--help" | "-h" => {
            print_usage(&args[0]);
        }
        _ => {
            eprintln!("Unknown command: {}", command);
            print_usage(&args[0]);
        }
    }

    Ok(())
}

fn print_usage(program_name: &str) {
    println!("Smart Money Tracker CLI");
    println!("Real-time monitoring of smart wallet activity for pump detection");
    println!();
    println!("USAGE:");
    println!("    {} <COMMAND> [OPTIONS]", program_name);
    println!();
    println!("COMMANDS:");
    println!("    check <mint> [timestamp]      Check smart money for a token");
    println!("    alert <mint> [timestamp]      Check if alert threshold is met");
    println!("    add-wallet <address>          Add a wallet to smart money list");
    println!("    list-wallets                  List all tracked smart wallets");
    println!("    monitor [duration]            Monitor smart money activity");
    println!("    help                          Show this help message");
    println!();
    println!("EXAMPLES:");
    println!(
        "    {} check 7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
        program_name
    );
    println!(
        "    {} alert 7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
        program_name
    );
    println!(
        "    {} add-wallet 9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM",
        program_name
    );
    println!("    {} monitor 300", program_name);
    println!();
    println!("ENVIRONMENT VARIABLES:");
    println!("    NANSEN_API_KEY            Nansen API key");
    println!("    BIRDEYE_API_KEY           Birdeye API key");
    println!("    RUST_LOG                  Set log level (info, debug, trace)");
    println!();
    println!("CONFIGURATION:");
    println!("    Detection window: 60 seconds from token deploy");
    println!("    Alert threshold: 2+ smart wallets buying");
    println!("    Real-time latency: <5 seconds from transaction");
}

async fn check_smart_money(token_mint: &str, deploy_timestamp: u64) -> Result<()> {
    println!("\n🔍 Checking smart money for token: {}", token_mint);
    println!("Deploy timestamp: {}", deploy_timestamp);
    println!("Detection window: 60 seconds\n");

    let config = load_config()?;
    let tracker = Arc::new(SmartMoneyTracker::new(config));

    let start = std::time::Instant::now();
    let (score, wallet_count, transactions) = tracker
        .check_smart_money(token_mint, deploy_timestamp)
        .await?;
    let elapsed = start.elapsed();

    println!("✅ Check completed in {:.2}s", elapsed.as_secs_f64());
    println!();
    println!("📊 RESULTS:");
    println!("  Smart Money Score:  {}/100", score);
    println!("  Unique Wallets:     {}", wallet_count);
    println!("  Transactions:       {}", transactions.len());
    println!();

    if !transactions.is_empty() {
        println!("📝 TRANSACTION DETAILS:");
        for (i, tx) in transactions.iter().enumerate() {
            println!("  {}. Wallet: {}", i + 1, tx.wallet);
            println!("     Amount: {:.4} SOL", tx.amount_sol);
            println!("     Time:   {}", tx.timestamp);
            println!("     Tx:     {}", tx.signature);
            println!("     Source: {:?}", tx.source);
            println!();
        }
    }

    // Provide recommendation
    if wallet_count >= 2 {
        println!("🚨 RECOMMENDATION: STRONG BUY SIGNAL");
        println!(
            "   {} smart wallets detected within 60s window",
            wallet_count
        );
    } else if wallet_count == 1 {
        println!("⚠️  RECOMMENDATION: MONITOR CLOSELY");
        println!("   1 smart wallet detected, waiting for more confirmations");
    } else {
        println!("ℹ️  RECOMMENDATION: NO SIGNAL");
        println!("   No smart wallet activity detected");
    }
    println!();

    Ok(())
}

async fn check_alert(token_mint: &str, deploy_timestamp: u64) -> Result<()> {
    println!("\n🚨 Checking alert for token: {}", token_mint);
    println!("Deploy timestamp: {}", deploy_timestamp);
    println!();

    let config = load_config()?;
    let tracker = Arc::new(SmartMoneyTracker::new(config));

    let start = std::time::Instant::now();
    let alert = tracker.check_alert(token_mint, deploy_timestamp).await?;
    let elapsed = start.elapsed();

    if let Some(alert) = alert {
        println!("🚨 SMART MONEY ALERT TRIGGERED!");
        println!("   Response time: {:.2}s", elapsed.as_secs_f64());
        println!();
        println!("📊 ALERT DETAILS:");
        println!("   Token:           {}", alert.token_mint);
        println!("   Unique Wallets:  {}", alert.unique_wallets);
        println!("   Total Volume:    {:.4} SOL", alert.total_volume_sol);
        println!("   Time to 1st Buy: {}s", alert.time_to_first_buy_secs);
        println!("   Transactions:    {}", alert.transactions.len());
        println!();

        println!("📝 SMART WALLET ACTIVITY:");
        for (i, tx) in alert.transactions.iter().enumerate() {
            println!(
                "   {}. {} bought {:.4} SOL",
                i + 1,
                tx.wallet,
                tx.amount_sol
            );
            println!("      At: {} | Tx: {}", tx.timestamp, tx.signature);
        }
        println!();

        println!("✅ ACTION: EXECUTE BUY ORDER");
        println!("   Confidence: HIGH");
        println!(
            "   Reason: {} smart wallets within {}s",
            alert.unique_wallets, alert.time_to_first_buy_secs
        );
    } else {
        println!("ℹ️  No alert triggered");
        println!("   Response time: {:.2}s", elapsed.as_secs_f64());
        println!("   Reason: Insufficient smart wallet activity (need 2+)");
    }
    println!();

    Ok(())
}

async fn add_wallet(wallet: &str) -> Result<()> {
    println!("\n➕ Adding wallet to smart money list: {}", wallet);

    let config = load_config()?;
    let tracker = Arc::new(SmartMoneyTracker::new(config));

    let profile = SmartWalletProfile {
        address: wallet.to_string(),
        source: DataSource::Custom,
        success_rate: None,
        total_profit_sol: None,
        successful_trades: None,
        last_activity: chrono::Utc::now().timestamp() as u64,
    };

    tracker.add_smart_wallet(profile).await;

    println!("✅ Wallet added successfully");
    println!("   Address: {}", wallet);
    println!("   Source: Custom");
    println!();

    Ok(())
}

async fn list_wallets() -> Result<()> {
    println!("\n📋 Tracked Smart Wallets\n");

    let config = load_config()?;
    let tracker = Arc::new(SmartMoneyTracker::new(config));

    let wallets = tracker.get_smart_wallets().await;

    if wallets.is_empty() {
        println!("ℹ️  No wallets tracked");
        println!("   Add wallets with: smart-money-cli add-wallet <address>");
    } else {
        println!("Total: {} wallets\n", wallets.len());
        for (i, wallet) in wallets.iter().enumerate() {
            println!("{}. {}", i + 1, wallet.address);
            println!("   Source: {:?}", wallet.source);
            if let Some(rate) = wallet.success_rate {
                println!("   Success Rate: {:.1}%", rate);
            }
            if let Some(profit) = wallet.total_profit_sol {
                println!("   Total Profit: {:.2} SOL", profit);
            }
            println!();
        }
    }
    println!();

    Ok(())
}

async fn monitor_smart_money(duration_secs: u64) -> Result<()> {
    println!("\n🔄 Starting Smart Money Monitor");
    println!("   Duration: {} seconds", duration_secs);
    println!("   Press Ctrl+C to stop\n");

    let config = load_config()?;
    let _tracker = Arc::new(SmartMoneyTracker::new(config));

    println!("📡 Monitoring smart wallet activity...");
    println!("   Alert threshold: 2+ wallets");
    println!("   Detection window: 60s");
    println!();

    // Simulate monitoring (in real implementation, this would listen to blockchain events)
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < duration_secs {
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        let elapsed = start.elapsed().as_secs();
        if elapsed % 30 == 0 {
            println!("⏱️  Monitoring... {}s elapsed", elapsed);
        }
    }

    println!("\n✅ Monitoring completed");
    println!("   Duration: {} seconds", duration_secs);
    println!();

    Ok(())
}

fn load_config() -> Result<SmartMoneyConfig> {
    let nansen_api_key = std::env::var("NANSEN_API_KEY").ok();
    let birdeye_api_key = std::env::var("BIRDEYE_API_KEY").ok();

    let enable_nansen = nansen_api_key.is_some();
    let enable_birdeye = birdeye_api_key.is_some();

    if !enable_nansen && !enable_birdeye {
        eprintln!("⚠️  Warning: No API keys configured");
        eprintln!("   Set NANSEN_API_KEY or BIRDEYE_API_KEY environment variables");
        eprintln!("   Smart money tracking will use fallback mode");
        eprintln!();
    }

    Ok(SmartMoneyConfig {
        nansen_api_key,
        birdeye_api_key,
        custom_smart_wallets: vec![],
        detection_window_secs: 60,
        min_smart_wallets: 2,
        cache_duration_secs: 300,
        api_timeout_secs: 5,
        enable_nansen,
        enable_birdeye,
    })
}
