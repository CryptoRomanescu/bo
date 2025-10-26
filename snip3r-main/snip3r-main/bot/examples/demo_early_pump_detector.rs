//! Demo: Early Pump Detector
//!
//! Demonstrates the ultra-fast early pump detection engine that makes
//! BUY/PASS decisions in <100 seconds from memecoin deployment.

use anyhow::Result;
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

    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║      Early Pump Detector - Demo & Benchmark               ║");
    println!("║  Ultra-fast memecoin analysis with <100s decisions        ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();

    // Demo 1: Single token analysis
    println!("=== DEMO 1: Single Token Analysis ===");
    println!();
    demo_single_analysis().await?;

    println!();
    println!("─────────────────────────────────────────────────────────────");
    println!();

    // Demo 2: Different platforms
    println!("=== DEMO 2: Multi-Platform Analysis ===");
    println!();
    demo_multi_platform().await?;

    println!();
    println!("─────────────────────────────────────────────────────────────");
    println!();

    // Demo 3: Parallel vs Sequential
    println!("=== DEMO 3: Parallel vs Sequential Performance ===");
    println!();
    demo_parallel_vs_sequential().await?;

    println!();
    println!("─────────────────────────────────────────────────────────────");
    println!();

    // Demo 4: Batch processing
    println!("=== DEMO 4: Batch Processing (100 tokens) ===");
    println!();
    demo_batch_processing().await?;

    println!();
    println!("═════════════════════════════════════════════════════════════");
    println!("✅ All demos completed successfully!");
    println!("═════════════════════════════════════════════════════════════");

    Ok(())
}

async fn demo_single_analysis() -> Result<()> {
    let config = EarlyPumpConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let detector = EarlyPumpDetector::new(config, rpc);

    let mint = "DemoToken1234567890ABC";
    let program = "pump.fun";
    let deploy_timestamp = chrono::Utc::now().timestamp() as u64 - 45; // 45 seconds ago

    println!("Analyzing token: {}", mint);
    println!("Platform: {}", program);
    println!("Deploy time: {} seconds ago", 45);
    println!();

    let result = detector.analyze(mint, deploy_timestamp, program).await?;

    print_analysis_result(&result);

    Ok(())
}

async fn demo_multi_platform() -> Result<()> {
    let config = EarlyPumpConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let detector = EarlyPumpDetector::new(config, rpc);

    let platforms = vec![
        ("pump.fun", "PumpFunToken123"),
        ("raydium", "RaydiumToken456"),
        ("jupiter", "JupiterToken789"),
        ("pumpswap", "PumpSwapTokenABC"),
    ];

    println!("Analyzing tokens from 4 different platforms:");
    println!();

    for (platform, mint) in platforms {
        let deploy_timestamp = chrono::Utc::now().timestamp() as u64 - 30;
        let result = detector.analyze(mint, deploy_timestamp, platform).await?;

        println!("Platform: {}", platform);
        println!("  Decision: {:?}", result.decision);
        println!("  Score: {}/100", result.score);
        println!("  Time: {}ms", result.timings.total_decision_time_ms);
        println!();
    }

    Ok(())
}

async fn demo_parallel_vs_sequential() -> Result<()> {
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));

    // Test parallel mode
    println!("Testing PARALLEL mode (5 iterations):");
    let config_parallel = EarlyPumpConfig {
        parallel_checks: true,
        ..Default::default()
    };
    let detector_parallel = EarlyPumpDetector::new(config_parallel, rpc.clone());

    let mut parallel_times = Vec::new();
    for i in 0..5 {
        let mint = format!("ParallelTest{}", i);
        let result = detector_parallel
            .analyze(
                &mint,
                chrono::Utc::now().timestamp() as u64 - 20,
                "pump.fun",
            )
            .await?;
        parallel_times.push(result.timings.total_decision_time_ms);
        println!(
            "  Iteration {}: {}ms",
            i + 1,
            result.timings.total_decision_time_ms
        );
    }

    let parallel_avg: u64 = parallel_times.iter().sum::<u64>() / parallel_times.len() as u64;
    println!("  Average: {}ms", parallel_avg);
    println!();

    // Test sequential mode
    println!("Testing SEQUENTIAL mode (5 iterations):");
    let config_sequential = EarlyPumpConfig {
        parallel_checks: false,
        ..Default::default()
    };
    let detector_sequential = EarlyPumpDetector::new(config_sequential, rpc);

    let mut sequential_times = Vec::new();
    for i in 0..5 {
        let mint = format!("SequentialTest{}", i);
        let result = detector_sequential
            .analyze(
                &mint,
                chrono::Utc::now().timestamp() as u64 - 20,
                "pump.fun",
            )
            .await?;
        sequential_times.push(result.timings.total_decision_time_ms);
        println!(
            "  Iteration {}: {}ms",
            i + 1,
            result.timings.total_decision_time_ms
        );
    }

    let sequential_avg: u64 = sequential_times.iter().sum::<u64>() / sequential_times.len() as u64;
    println!("  Average: {}ms", sequential_avg);
    println!();

    let speedup = sequential_avg as f64 / parallel_avg as f64;
    println!("📊 RESULTS:");
    println!("  Parallel:   {} ms", parallel_avg);
    println!("  Sequential: {} ms", sequential_avg);
    println!("  Speedup:    {:.2}x", speedup);

    Ok(())
}

async fn demo_batch_processing() -> Result<()> {
    let config = EarlyPumpConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let detector = Arc::new(EarlyPumpDetector::new(config, rpc));

    let batch_size = 100;
    let mut buy_count = 0;
    let mut pass_count = 0;
    let mut total_time_ms = 0u64;
    let mut under_100s = 0;

    println!("Processing {} tokens in parallel batches...", batch_size);
    println!();

    let start = std::time::Instant::now();

    // Process in batches of 20 for demonstration
    for batch in 0..(batch_size / 20) {
        let mut handles = Vec::new();

        for i in 0..20 {
            let detector = detector.clone();
            let token_id = batch * 20 + i;

            let handle = tokio::spawn(async move {
                let mint = format!("BatchToken{}", token_id);
                let program = match token_id % 4 {
                    0 => "pump.fun",
                    1 => "raydium",
                    2 => "jupiter",
                    _ => "pumpswap",
                };
                let deploy_timestamp =
                    chrono::Utc::now().timestamp() as u64 - ((token_id % 60) as u64);

                detector.analyze(&mint, deploy_timestamp, program).await
            });

            handles.push(handle);
        }

        // Wait for batch
        for handle in handles {
            if let Ok(Ok(result)) = handle.await {
                total_time_ms += result.timings.total_decision_time_ms;
                if result.timings.total_decision_time_ms < 100_000 {
                    under_100s += 1;
                }

                match result.decision {
                    PumpDecision::Buy { .. } => buy_count += 1,
                    PumpDecision::Pass { .. } => pass_count += 1,
                }
            }
        }

        print!(".");
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }

    println!();
    println!();

    let elapsed = start.elapsed();

    println!("📊 BATCH RESULTS:");
    println!("  Total tokens:       {}", batch_size);
    println!("  Wall time:          {:.2}s", elapsed.as_secs_f64());
    println!(
        "  Throughput:         {:.1} tokens/sec",
        batch_size as f64 / elapsed.as_secs_f64()
    );
    println!();
    println!(
        "  BUY decisions:      {} ({:.1}%)",
        buy_count,
        buy_count as f64 / batch_size as f64 * 100.0
    );
    println!(
        "  PASS decisions:     {} ({:.1}%)",
        pass_count,
        pass_count as f64 / batch_size as f64 * 100.0
    );
    println!();
    println!(
        "  Avg time/token:     {:.2}ms",
        total_time_ms as f64 / batch_size as f64
    );
    println!(
        "  Under 100s:         {} ({:.1}%)",
        under_100s,
        under_100s as f64 / batch_size as f64 * 100.0
    );
    println!();

    let success_rate = under_100s as f64 / batch_size as f64;
    if success_rate >= 0.95 {
        println!(
            "  ✅ SUCCESS: {:.1}% under 100s (target: 95%)",
            success_rate * 100.0
        );
    } else {
        println!(
            "  ⚠️  WARNING: Only {:.1}% under 100s (target: 95%)",
            success_rate * 100.0
        );
    }

    Ok(())
}

fn print_analysis_result(result: &h_5n1p3r::oracle::EarlyPumpAnalysis) {
    match &result.decision {
        PumpDecision::Buy { score, reason } => {
            println!("🟢 DECISION: BUY");
            println!("📊 Score: {}/100", score);
            println!("💡 Reason: {}", reason);
        }
        PumpDecision::Pass { score, reason } => {
            println!("🔴 DECISION: PASS");
            println!("📊 Score: {}/100", score);
            println!("💡 Reason: {}", reason);
        }
    }

    println!();
    println!("⏱️  TIMING:");
    println!(
        "  Deploy → Detection:   {}ms",
        result.timings.deploy_to_detection_ms
    );
    println!(
        "  Detection → Decision: {}ms",
        result.timings.detection_to_decision_ms
    );
    println!(
        "  Total:                {}ms",
        result.timings.total_decision_time_ms
    );

    if result.timings.total_decision_time_ms < 100_000 {
        println!("  ✅ Within 100s target!");
    } else {
        println!("  ⚠️  EXCEEDED 100s target");
    }

    println!();
    println!("📈 CHECK BREAKDOWN:");
    println!(
        "  Supply Concentration: {}/100 (lower is better)",
        result.check_results.supply_concentration
    );
    println!(
        "  LP Lock:              {}/100 (higher is better)",
        result.check_results.lp_lock
    );
    println!(
        "  Wash Trading Risk:    {}/100 (lower is better)",
        result.check_results.wash_trading_risk
    );
    println!(
        "  Smart Money:          {}/100 (higher is better)",
        result.check_results.smart_money
    );
    println!(
        "  Holder Growth:        {}/100 (higher is better)",
        result.check_results.holder_growth
    );

    println!();
    println!("⏱️  CHECK TIMINGS:");
    println!(
        "  Supply Concentration: {}ms",
        result.timings.check_timings.supply_concentration_ms
    );
    println!(
        "  LP Lock:              {}ms",
        result.timings.check_timings.lp_lock_ms
    );
    println!(
        "  Wash Trading:         {}ms",
        result.timings.check_timings.wash_trading_ms
    );
    println!(
        "  Smart Money:          {}ms",
        result.timings.check_timings.smart_money_ms
    );
    println!(
        "  Holder Growth:        {}ms",
        result.timings.check_timings.holder_growth_ms
    );
}
