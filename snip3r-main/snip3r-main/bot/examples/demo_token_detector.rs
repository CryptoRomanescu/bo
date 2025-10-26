//! Demo application for Ultra-Fast Token Detection System
//!
//! This example demonstrates:
//! - Multi-source token detection
//! - Sub-second latency tracking
//! - Real-time metrics monitoring
//! - Integration with decision engine

use h_5n1p3r::oracle::{DetectionEvent, DetectionSource, TokenDetector, TokenDetectorConfig};
use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::time::{interval, sleep};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Ultra-Fast Token Detection System Demo ===\n");

    // Create channel for detected tokens
    let (detector_tx, mut detector_rx) = mpsc::channel(100);

    // Configure detection system
    let mut config = TokenDetectorConfig::default();

    // Enable multiple sources
    config.enabled_sources = HashSet::new();
    config.enabled_sources.insert(DetectionSource::PumpFun);
    config.enabled_sources.insert(DetectionSource::Raydium);
    config.enabled_sources.insert(DetectionSource::Jupiter);

    // Set aggressive deduplication (250ms window)
    config.dedup_window_ms = 250;

    println!("Configuration:");
    println!("  Enabled Sources: {:?}", config.enabled_sources);
    println!("  Dedup Window: {}ms", config.dedup_window_ms);
    println!(
        "  Max Clock Skew: {}s",
        config.sensitivity.max_clock_skew_secs
    );
    println!();

    // Create token detector
    let detector = TokenDetector::new(config, detector_tx);

    println!("Token Detector initialized\n");

    // Simulate detection events from multiple sources
    let detector_clone = detector.clone();
    tokio::spawn(async move {
        simulate_detections(detector_clone).await;
    });

    // Monitor detected tokens
    let detector_clone = detector.clone();
    tokio::spawn(async move {
        monitor_detections(detector_clone).await;
    });

    // Process detected tokens
    println!("=== Monitoring Token Launches ===\n");
    let mut token_count = 0;

    while let Some(candidate) = detector_rx.recv().await {
        token_count += 1;

        println!("✓ Token Detected #{}", token_count);
        println!("  Mint: {}", candidate.mint);
        println!("  Creator: {}", candidate.creator);
        println!("  Program: {}", candidate.program);
        println!("  Slot: {}", candidate.slot);
        println!("  Timestamp: {}", candidate.timestamp);

        if let Some(ref summary) = candidate.instruction_summary {
            println!("  Instruction: {}", summary);
        }

        if let Some(jito) = candidate.is_jito_bundle {
            println!("  Jito Bundle: {}", jito);
        }

        println!();

        // Limit demo to 10 detections
        if token_count >= 10 {
            break;
        }
    }

    // Final metrics report
    println!("\n=== Final Detection Metrics ===\n");
    let metrics = detector.get_metrics().await;

    println!("Total Events: {}", metrics.total_events);
    println!("Forwarded Events: {}", metrics.forwarded_events);
    println!("Duplicate Events: {}", metrics.duplicate_count);
    println!();

    if let Some(p50) = metrics.p50_latency_ms() {
        println!("P50 Latency: {}ms", p50);
    }
    if let Some(p95) = metrics.p95_latency_ms() {
        println!("P95 Latency: {}ms", p95);
    }
    if let Some(p99) = metrics.p99_latency_ms() {
        println!("P99 Latency: {}ms", p99);
    }

    println!("\nDetection Accuracy: {:.2}%", metrics.detection_accuracy());
    println!();

    println!("Detections by Source:");
    for source in [
        DetectionSource::PumpFun,
        DetectionSource::Raydium,
        DetectionSource::Jupiter,
    ] {
        let count = metrics.detection_rate_by_source(source);
        if count > 0 {
            println!("  {}: {}", source.name(), count);
        }
    }

    println!("\n=== Demo Complete ===");

    Ok(())
}

/// Simulate token detection events from multiple sources
async fn simulate_detections(detector: TokenDetector) {
    let mut ticker = interval(Duration::from_millis(500));

    let sources = vec![
        DetectionSource::PumpFun,
        DetectionSource::Raydium,
        DetectionSource::Jupiter,
    ];

    for i in 0..15 {
        ticker.tick().await;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Vary latency (100-800ms)
        let latency_ms = 100 + (i * 50);
        let deploy_timestamp = now.saturating_sub(latency_ms / 1000);

        let source = sources[i as usize % sources.len()];

        let event = DetectionEvent {
            mint: format!("Mint{:04}...{:04}", i, 9999 - i),
            creator: format!("Creator{:03}", i),
            program: source.name().to_lowercase(),
            slot: 100000 + i,
            deploy_timestamp,
            detection_timestamp: now,
            source,
            instruction_summary: Some(format!("{} launch", source.name())),
            is_jito_bundle: Some(i % 3 == 0),
            metadata: std::collections::HashMap::new(),
        };

        if let Err(e) = detector.process_event(event).await {
            eprintln!("Error processing event: {}", e);
        }

        // Occasionally send duplicate to test deduplication
        if i % 5 == 0 && i > 0 {
            sleep(Duration::from_millis(100)).await;

            let duplicate_event = DetectionEvent {
                mint: format!("Mint{:04}...{:04}", i - 1, 9999 - (i - 1)),
                creator: format!("Creator{:03}", i - 1),
                program: "raydium".to_string(),
                slot: 100000 + i - 1,
                deploy_timestamp: now.saturating_sub(1),
                detection_timestamp: now,
                source: DetectionSource::Raydium,
                instruction_summary: Some("duplicate".to_string()),
                is_jito_bundle: None,
                metadata: std::collections::HashMap::new(),
            };

            let _ = detector.process_event(duplicate_event).await;
        }
    }
}

/// Monitor and display real-time detection metrics
async fn monitor_detections(detector: TokenDetector) {
    let mut ticker = interval(Duration::from_secs(3));

    for _ in 0..5 {
        ticker.tick().await;

        let metrics = detector.get_metrics().await;

        if metrics.total_events > 0 {
            println!("--- Live Metrics ---");
            println!(
                "  Events: {} (Forwarded: {}, Duplicates: {})",
                metrics.total_events, metrics.forwarded_events, metrics.duplicate_count
            );

            if let Some(p95) = metrics.p95_latency_ms() {
                println!("  P95 Latency: {}ms", p95);
            }

            println!("  Accuracy: {:.1}%", metrics.detection_accuracy());
            println!();
        }
    }
}
