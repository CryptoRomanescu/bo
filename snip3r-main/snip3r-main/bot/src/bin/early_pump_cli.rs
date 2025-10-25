//! CLI for Early Pump Detector
//!
//! Provides command-line interface for analyzing memecoin launches
//! and making BUY/PASS decisions in <100 seconds

use anyhow::{Context, Result};
use h_5n1p3r::oracle::{EarlyPumpConfig, EarlyPumpDetector, PumpDecision};
use solana_client::nonblocking::rpc_client::RpcClient;
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
        "analyze" => {
            if args.len() < 4 {
                eprintln!("Usage: {} analyze <mint_address> <program> [deploy_timestamp]", args[0]);
                eprintln!("Example: {} analyze 7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU pump.fun", args[0]);
                return Ok(());
            }

            let mint = &args[2];
            let program = &args[3];
            let deploy_timestamp = if args.len() > 4 {
                args[4].parse::<u64>().context("Invalid timestamp")?
            } else {
                // Default to current time minus 30 seconds
                chrono::Utc::now().timestamp() as u64 - 30
            };

            analyze_token(mint, program, deploy_timestamp).await?;
        }
        "batch" => {
            if args.len() < 2 {
                eprintln!("Usage: {} batch <count>", args[0]);
                eprintln!("Example: {} batch 100", args[0]);
                return Ok(());
            }

            let count = args.get(2)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(10);

            batch_analyze(count).await?;
        }
        "benchmark" => {
            benchmark().await?;
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
    println!("Early Pump Detector CLI");
    println!("Ultra-fast memecoin analysis with <100s decision time");
    println!();
    println!("USAGE:");
    println!("    {} <COMMAND> [OPTIONS]", program_name);
    println!();
    println!("COMMANDS:");
    println!("    analyze <mint> <program> [timestamp]  Analyze a single token");
    println!("    batch <count>                          Analyze multiple tokens");
    println!("    benchmark                              Run performance benchmark");
    println!("    help                                   Show this help message");
    println!();
    println!("EXAMPLES:");
    println!("    {} analyze 7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU pump.fun", program_name);
    println!("    {} batch 100", program_name);
    println!("    {} benchmark", program_name);
    println!();
    println!("ENVIRONMENT VARIABLES:");
    println!("    RPC_URL                Set custom Solana RPC endpoint");
    println!("    RUST_LOG               Set log level (info, debug, trace)");
}

async fn analyze_token(mint: &str, program: &str, deploy_timestamp: u64) -> Result<()> {
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║         Early Pump Detector - Token Analysis              ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();
    println!("Mint:              {}", mint);
    println!("Program:           {}", program);
    println!("Deploy timestamp:  {}", deploy_timestamp);
    println!();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());

    let config = EarlyPumpConfig {
        detection_timeout_secs: 60,
        decision_timeout_secs: 60,
        buy_threshold: 70,
        pass_threshold: 50,
        parallel_checks: true,
        rpc_endpoints: vec![rpc_url.clone()],
    };

    let rpc = Arc::new(RpcClient::new(rpc_url));
    let detector = EarlyPumpDetector::new(config, rpc);

    println!("⏳ Analyzing token (this should take <100 seconds)...");
    println!();

    let result = detector
        .analyze(mint, deploy_timestamp, program)
        .await
        .context("Analysis failed")?;

    // Print results
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║                      ANALYSIS RESULTS                      ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();

    // Decision
    match &result.decision {
        PumpDecision::Buy { score, reason } => {
            println!("🟢 DECISION:  BUY");
            println!("📊 SCORE:     {}/100", score);
            println!("💡 REASON:    {}", reason);
        }
        PumpDecision::Pass { score, reason } => {
            println!("🔴 DECISION:  PASS");
            println!("📊 SCORE:     {}/100", score);
            println!("💡 REASON:    {}", reason);
        }
    }
    println!();

    // Timing
    println!("⏱️  TIMING:");
    println!("   Deploy → Detection:    {} ms", result.timings.deploy_to_detection_ms);
    println!("   Detection → Decision:  {} ms", result.timings.detection_to_decision_ms);
    println!("   Total:                 {} ms", result.timings.total_decision_time_ms);
    
    if result.timings.total_decision_time_ms < 100_000 {
        println!("   ✅ Within 100s target!");
    } else {
        println!("   ⚠️  EXCEEDED 100s target");
    }
    println!();

    // Check results
    println!("📈 CHECK BREAKDOWN:");
    println!("   Supply Concentration:  {}/100 (lower is better)", result.check_results.supply_concentration);
    println!("   LP Lock:               {}/100 (higher is better)", result.check_results.lp_lock);
    println!("   Wash Trading Risk:     {}/100 (lower is better)", result.check_results.wash_trading_risk);
    println!("   Smart Money:           {}/100 (higher is better)", result.check_results.smart_money);
    println!("   Holder Growth:         {}/100 (higher is better)", result.check_results.holder_growth);
    println!();

    // Individual timing breakdown
    println!("⏱️  CHECK TIMINGS:");
    println!("   Supply Concentration:  {} ms", result.timings.check_timings.supply_concentration_ms);
    println!("   LP Lock:               {} ms", result.timings.check_timings.lp_lock_ms);
    println!("   Wash Trading:          {} ms", result.timings.check_timings.wash_trading_ms);
    println!("   Smart Money:           {} ms", result.timings.check_timings.smart_money_ms);
    println!("   Holder Growth:         {} ms", result.timings.check_timings.holder_growth_ms);
    println!();

    // Export as JSON
    let json = serde_json::to_string_pretty(&result)?;
    println!("📄 JSON OUTPUT:");
    println!("{}", json);

    Ok(())
}

async fn batch_analyze(count: usize) -> Result<()> {
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║         Early Pump Detector - Batch Analysis              ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();
    println!("Analyzing {} simulated tokens...", count);
    println!();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());

    let config = EarlyPumpConfig::default();
    let rpc = Arc::new(RpcClient::new(rpc_url));
    let detector = Arc::new(EarlyPumpDetector::new(config, rpc));

    let mut buy_count = 0;
    let mut pass_count = 0;
    let mut total_time_ms = 0u64;
    let mut under_100s = 0;

    let start = std::time::Instant::now();

    for i in 0..count {
        let mint = format!("SimulatedMint{}", i);
        let program = match i % 4 {
            0 => "pump.fun",
            1 => "raydium",
            2 => "jupiter",
            _ => "pumpswap",
        };
        let deploy_timestamp = chrono::Utc::now().timestamp() as u64 - ((i % 60) as u64);

        let result = detector.analyze(&mint, deploy_timestamp, program).await?;

        total_time_ms += result.timings.total_decision_time_ms;
        if result.timings.total_decision_time_ms < 100_000 {
            under_100s += 1;
        }

        match result.decision {
            PumpDecision::Buy { .. } => buy_count += 1,
            PumpDecision::Pass { .. } => pass_count += 1,
        }

        if (i + 1) % 10 == 0 {
            println!("Progress: {}/{} tokens analyzed", i + 1, count);
        }
    }

    let total_elapsed = start.elapsed();

    println!();
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║                    BATCH RESULTS                           ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();
    println!("📊 STATISTICS:");
    println!("   Total tokens:          {}", count);
    println!("   BUY decisions:         {} ({:.1}%)", buy_count, buy_count as f64 / count as f64 * 100.0);
    println!("   PASS decisions:        {} ({:.1}%)", pass_count, pass_count as f64 / count as f64 * 100.0);
    println!("   Under 100s:            {} ({:.1}%)", under_100s, under_100s as f64 / count as f64 * 100.0);
    println!();
    println!("⏱️  TIMING:");
    println!("   Total wall time:       {:.2}s", total_elapsed.as_secs_f64());
    println!("   Avg time per token:    {:.2}ms", total_time_ms as f64 / count as f64);
    println!();

    let success_rate = under_100s as f64 / count as f64;
    if success_rate >= 0.95 {
        println!("✅ SUCCESS: {:.1}% under 100s (target: 95%)", success_rate * 100.0);
    } else {
        println!("⚠️  WARNING: Only {:.1}% under 100s (target: 95%)", success_rate * 100.0);
    }

    Ok(())
}

async fn benchmark() -> Result<()> {
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║       Early Pump Detector - Performance Benchmark         ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());

    // Test different configurations
    let configs = vec![
        ("Parallel (default)", EarlyPumpConfig {
            parallel_checks: true,
            ..Default::default()
        }),
        ("Sequential", EarlyPumpConfig {
            parallel_checks: false,
            ..Default::default()
        }),
        ("Aggressive (30s timeout)", EarlyPumpConfig {
            detection_timeout_secs: 30,
            decision_timeout_secs: 30,
            parallel_checks: true,
            ..Default::default()
        }),
    ];

    for (name, config) in configs {
        println!("Testing: {}", name);
        let rpc = Arc::new(RpcClient::new(rpc_url.clone()));
        let detector = EarlyPumpDetector::new(config, rpc);

        let iterations = 20;
        let mut timings = Vec::new();

        for i in 0..iterations {
            let mint = format!("BenchMint{}", i);
            let result = detector
                .analyze(&mint, chrono::Utc::now().timestamp() as u64 - 20, "pump.fun")
                .await?;
            timings.push(result.timings.total_decision_time_ms);
        }

        timings.sort();
        let min = timings[0];
        let max = timings[timings.len() - 1];
        let median = timings[timings.len() / 2];
        let avg: u64 = timings.iter().sum::<u64>() / timings.len() as u64;

        println!("  Min:    {} ms", min);
        println!("  Median: {} ms", median);
        println!("  Avg:    {} ms", avg);
        println!("  Max:    {} ms", max);
        println!();
    }

    println!("✅ Benchmark complete!");

    Ok(())
}
