//! Integration tests for Early Pump Detector
//!
//! Validates the <100s decision time requirement with simulated memecoin launches

use h_5n1p3r::oracle::{EarlyPumpConfig, EarlyPumpDetector, PumpDecision};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use std::time::Instant;

#[tokio::test]
async fn test_single_decision_timing() {
    let config = EarlyPumpConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let detector = EarlyPumpDetector::new(config, rpc);

    let deploy_timestamp = chrono::Utc::now().timestamp() as u64 - 30; // 30 seconds ago
    let start = Instant::now();

    let result = detector
        .analyze("SingleTestMint", deploy_timestamp, "pump.fun")
        .await
        .expect("Analysis should succeed");

    let elapsed = start.elapsed().as_millis();

    println!("Single decision completed in {}ms", elapsed);
    println!("Decision: {:?}", result.decision);
    println!("Score: {}", result.score);

    // Verify timing requirements
    assert!(
        result.timings.total_decision_time_ms < 100_000,
        "Total decision time {}ms exceeds 100s limit",
        result.timings.total_decision_time_ms
    );

    assert!(
        result.timings.detection_to_decision_ms < 60_000,
        "Decision phase {}ms exceeds 60s limit",
        result.timings.detection_to_decision_ms
    );
}

#[tokio::test]
async fn test_parallel_vs_sequential() {
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));

    // Test parallel mode
    let config_parallel = EarlyPumpConfig {
        parallel_checks: true,
        ..Default::default()
    };
    let detector_parallel = EarlyPumpDetector::new(config_parallel, rpc.clone());

    let start_parallel = Instant::now();
    let result_parallel = detector_parallel
        .analyze(
            "ParallelTestMint",
            chrono::Utc::now().timestamp() as u64,
            "pump.fun",
        )
        .await
        .unwrap();
    let time_parallel = start_parallel.elapsed().as_millis();

    // Test sequential mode
    let config_sequential = EarlyPumpConfig {
        parallel_checks: false,
        ..Default::default()
    };
    let detector_sequential = EarlyPumpDetector::new(config_sequential, rpc);

    let start_sequential = Instant::now();
    let result_sequential = detector_sequential
        .analyze(
            "SequentialTestMint",
            chrono::Utc::now().timestamp() as u64,
            "pump.fun",
        )
        .await
        .unwrap();
    let time_sequential = start_sequential.elapsed().as_millis();

    println!("Parallel mode: {}ms", time_parallel);
    println!("Sequential mode: {}ms", time_sequential);

    // Parallel should be faster
    assert!(
        time_parallel < time_sequential,
        "Parallel mode should be faster than sequential"
    );

    // Both should meet timing requirements
    assert!(time_parallel < 100_000);
    assert!(time_sequential < 100_000);

    // Verify results are valid
    assert!(result_parallel.score <= 100);
    assert!(result_sequential.score <= 100);
}

#[tokio::test]
async fn test_decision_thresholds() {
    let config = EarlyPumpConfig {
        buy_threshold: 70,
        pass_threshold: 50,
        ..Default::default()
    };
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let detector = EarlyPumpDetector::new(config, rpc);

    let result = detector
        .analyze(
            "ThresholdTestMint",
            chrono::Utc::now().timestamp() as u64,
            "pump.fun",
        )
        .await
        .unwrap();

    // Verify decision is either BUY or PASS
    match result.decision {
        PumpDecision::Buy { score, reason } => {
            println!("BUY decision: score={}, reason={}", score, reason);
            assert_eq!(score, result.score);
        }
        PumpDecision::Pass { score, reason } => {
            println!("PASS decision: score={}, reason={}", score, reason);
            assert_eq!(score, result.score);
        }
    }
}

#[tokio::test]
async fn test_1000_memecoin_launches() {
    let config = EarlyPumpConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let detector = Arc::new(EarlyPumpDetector::new(config, rpc));

    let total_launches = 1000;
    let mut timings = Vec::with_capacity(total_launches);
    let mut decisions_buy = 0;
    let mut decisions_pass = 0;
    let mut decisions_under_100s = 0;

    println!("Simulating {} memecoin launches...", total_launches);
    let overall_start = Instant::now();

    // Run launches in batches to simulate realistic conditions
    let batch_size = 50;
    for batch in 0..(total_launches / batch_size) {
        let mut handles = Vec::new();

        for i in 0..batch_size {
            let detector = detector.clone();
            let launch_id = batch * batch_size + i;

            let handle = tokio::spawn(async move {
                let mint = format!("SimulatedMint{}", launch_id);
                let deploy_timestamp =
                    chrono::Utc::now().timestamp() as u64 - (launch_id % 60) as u64;
                let program = if launch_id % 4 == 0 {
                    "pump.fun"
                } else if launch_id % 4 == 1 {
                    "raydium"
                } else if launch_id % 4 == 2 {
                    "jupiter"
                } else {
                    "pumpswap"
                };

                detector.analyze(&mint, deploy_timestamp, program).await
            });

            handles.push(handle);
        }

        // Wait for batch to complete
        for handle in handles {
            match handle.await {
                Ok(Ok(result)) => {
                    timings.push(result.timings.total_decision_time_ms);

                    if result.timings.total_decision_time_ms < 100_000 {
                        decisions_under_100s += 1;
                    }

                    match result.decision {
                        PumpDecision::Buy { .. } => decisions_buy += 1,
                        PumpDecision::Pass { .. } => decisions_pass += 1,
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("Analysis failed: {}", e);
                }
                Err(e) => {
                    eprintln!("Task panicked: {}", e);
                }
            }
        }

        if (batch + 1) % 5 == 0 {
            println!(
                "Progress: {}/{} launches completed",
                (batch + 1) * batch_size,
                total_launches
            );
        }
    }

    let overall_elapsed = overall_start.elapsed();
    println!("\n=== Simulation Results ===");
    println!("Total launches: {}", total_launches);
    println!("Total time: {:?}", overall_elapsed);
    println!(
        "Average time per launch: {:.2}ms",
        overall_elapsed.as_millis() as f64 / total_launches as f64
    );
    println!("\n=== Decision Statistics ===");
    println!(
        "BUY decisions: {} ({:.1}%)",
        decisions_buy,
        decisions_buy as f64 / total_launches as f64 * 100.0
    );
    println!(
        "PASS decisions: {} ({:.1}%)",
        decisions_pass,
        decisions_pass as f64 / total_launches as f64 * 100.0
    );
    println!(
        "Decisions under 100s: {} ({:.1}%)",
        decisions_under_100s,
        decisions_under_100s as f64 / total_launches as f64 * 100.0
    );

    // Calculate timing statistics
    if !timings.is_empty() {
        timings.sort();
        let min_time = timings[0];
        let max_time = timings[timings.len() - 1];
        let median_time = timings[timings.len() / 2];
        let avg_time: u64 = timings.iter().sum::<u64>() / timings.len() as u64;
        let p95_time = timings[(timings.len() as f64 * 0.95) as usize];
        let p99_time = timings[(timings.len() as f64 * 0.99) as usize];

        println!("\n=== Timing Statistics ===");
        println!("Min: {}ms", min_time);
        println!("Median: {}ms", median_time);
        println!("Average: {}ms", avg_time);
        println!("P95: {}ms", p95_time);
        println!("P99: {}ms", p99_time);
        println!("Max: {}ms", max_time);

        // Acceptance criteria: 95% of decisions should be under 100s
        let success_rate = decisions_under_100s as f64 / total_launches as f64;
        println!("\n=== Acceptance Criteria ===");
        println!("Target: 95% decisions under 100s");
        println!("Actual: {:.1}% decisions under 100s", success_rate * 100.0);

        assert!(
            success_rate >= 0.95,
            "FAILED: Only {:.1}% decisions under 100s (need 95%)",
            success_rate * 100.0
        );

        println!("✅ PASSED: Acceptance criteria met!");
    }
}

#[tokio::test]
async fn test_different_programs() {
    let config = EarlyPumpConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let detector = EarlyPumpDetector::new(config, rpc);

    let programs = vec!["pump.fun", "raydium", "jupiter", "pumpswap"];
    let deploy_timestamp = chrono::Utc::now().timestamp() as u64 - 20;

    for program in programs {
        let mint = format!("Test-{}-Mint", program);
        let result = detector.analyze(&mint, deploy_timestamp, program).await;

        assert!(result.is_ok(), "Analysis failed for program: {}", program);

        let analysis = result.unwrap();
        println!(
            "Program: {}, Decision: {:?}, Score: {}, Time: {}ms",
            program, analysis.decision, analysis.score, analysis.timings.total_decision_time_ms
        );

        // All should complete within time limit
        assert!(analysis.timings.total_decision_time_ms < 100_000);
    }
}

#[tokio::test]
async fn test_check_timings_breakdown() {
    let config = EarlyPumpConfig {
        parallel_checks: true,
        ..Default::default()
    };
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let detector = EarlyPumpDetector::new(config, rpc);

    let result = detector
        .analyze(
            "TimingTestMint",
            chrono::Utc::now().timestamp() as u64,
            "pump.fun",
        )
        .await
        .unwrap();

    let timings = &result.timings.check_timings;

    println!("=== Individual Check Timings ===");
    println!(
        "Supply concentration: {}ms",
        timings.supply_concentration_ms
    );
    println!("LP lock: {}ms", timings.lp_lock_ms);
    println!("Wash trading: {}ms", timings.wash_trading_ms);
    println!("Smart money: {}ms", timings.smart_money_ms);
    println!("Holder growth: {}ms", timings.holder_growth_ms);

    // All individual checks should complete reasonably fast
    assert!(timings.supply_concentration_ms < 10_000);
    assert!(timings.lp_lock_ms < 10_000);
    assert!(timings.wash_trading_ms < 10_000);
    assert!(timings.smart_money_ms < 10_000);
    assert!(timings.holder_growth_ms < 10_000);
}

#[tokio::test]
async fn test_detection_latency_tracking() {
    let config = EarlyPumpConfig::default();
    let rpc = Arc::new(RpcClient::new(
        "https://api.mainnet-beta.solana.com".to_string(),
    ));
    let detector = EarlyPumpDetector::new(config, rpc);

    // Simulate various detection latencies
    let test_cases = vec![
        ("Early detection", 5),   // 5 seconds after deploy
        ("Medium detection", 30), // 30 seconds after deploy
        ("Late detection", 55),   // 55 seconds after deploy
    ];

    for (name, delay) in test_cases {
        let deploy_timestamp = chrono::Utc::now().timestamp() as u64 - delay;
        let result = detector
            .analyze(&format!("Latency-{}", name), deploy_timestamp, "pump.fun")
            .await
            .unwrap();

        println!(
            "{}: deploy_to_detection={}ms, detection_to_decision={}ms, total={}ms",
            name,
            result.timings.deploy_to_detection_ms,
            result.timings.detection_to_decision_ms,
            result.timings.total_decision_time_ms
        );

        // Verify timing fields are populated correctly
        assert!(result.timings.deploy_to_detection_ms > 0);
        // detection_to_decision_ms might be 0 for very fast operations (sub-second)
        // since it's calculated from timestamps with second precision
        // The actual processing time is captured in total_decision_time_ms
        assert!(result.timings.total_decision_time_ms > 0);
    }
}
