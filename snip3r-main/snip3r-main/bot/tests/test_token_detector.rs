//! Integration tests for the Ultra-Fast Token Detection System

use h_5n1p3r::oracle::{
    DetectionEvent, DetectionMetrics, DetectionSensitivity, DetectionSource, TokenDetector,
    TokenDetectorConfig,
};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

#[tokio::test]
async fn test_token_detector_basic_detection() {
    let (tx, mut rx) = mpsc::channel(100);
    let config = TokenDetectorConfig::default();
    let detector = TokenDetector::new(config, tx);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let event = DetectionEvent {
        mint: "test_mint_123".to_string(),
        creator: "creator_456".to_string(),
        program: "pump.fun".to_string(),
        slot: 100000,
        deploy_timestamp: now - 1,
        detection_timestamp: now,
        source: DetectionSource::PumpFun,
        instruction_summary: Some("initialize".to_string()),
        is_jito_bundle: Some(true),
        metadata: HashMap::new(),
    };

    // Process event
    detector.process_event(event.clone()).await.unwrap();

    // Should receive forwarded candidate
    let candidate = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("Timeout waiting for candidate")
        .expect("Channel closed");

    assert_eq!(candidate.mint, "test_mint_123");
    assert_eq!(candidate.creator, "creator_456");
    assert_eq!(candidate.program, "pump.fun");

    // Check metrics
    let metrics = detector.get_metrics().await;
    assert_eq!(metrics.total_events, 1);
    assert_eq!(metrics.forwarded_events, 1);
    assert_eq!(metrics.duplicate_count, 0);
}

#[tokio::test]
async fn test_token_detector_deduplication() {
    let (tx, mut rx) = mpsc::channel(100);
    let config = TokenDetectorConfig::default();
    let detector = TokenDetector::new(config, tx);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let event1 = DetectionEvent {
        mint: "duplicate_mint".to_string(),
        creator: "creator".to_string(),
        program: "pump.fun".to_string(),
        slot: 100000,
        deploy_timestamp: now - 1,
        detection_timestamp: now,
        source: DetectionSource::PumpFun,
        instruction_summary: None,
        is_jito_bundle: None,
        metadata: HashMap::new(),
    };

    let event2 = DetectionEvent {
        mint: "duplicate_mint".to_string(),
        creator: "creator".to_string(),
        program: "pump.fun".to_string(),
        slot: 100001,
        deploy_timestamp: now - 1,
        detection_timestamp: now,
        source: DetectionSource::Raydium, // Different source
        instruction_summary: None,
        is_jito_bundle: None,
        metadata: HashMap::new(),
    };

    // Process both events
    detector.process_event(event1).await.unwrap();
    detector.process_event(event2).await.unwrap();

    // Should only receive one candidate (first one)
    let candidate = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("Timeout waiting for first candidate")
        .expect("Channel closed");

    assert_eq!(candidate.mint, "duplicate_mint");

    // Second should not be received (filtered as duplicate)
    let result = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
    assert!(result.is_err(), "Should not receive duplicate");

    // Check metrics
    let metrics = detector.get_metrics().await;
    assert_eq!(metrics.total_events, 1, "Only first event counted");
    assert_eq!(metrics.duplicate_count, 1, "Second was duplicate");
    assert_eq!(metrics.forwarded_events, 1, "Only first forwarded");
}

#[tokio::test]
async fn test_token_detector_multiple_sources() {
    let (tx, mut rx) = mpsc::channel(100);
    let mut config = TokenDetectorConfig::default();

    // Enable multiple sources
    config.enabled_sources.insert(DetectionSource::PumpFun);
    config.enabled_sources.insert(DetectionSource::Raydium);
    config.enabled_sources.insert(DetectionSource::Jupiter);

    let detector = TokenDetector::new(config, tx);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Create events from different sources
    let sources = vec![
        DetectionSource::PumpFun,
        DetectionSource::Raydium,
        DetectionSource::Jupiter,
    ];

    for (i, source) in sources.iter().enumerate() {
        let event = DetectionEvent {
            mint: format!("mint_{}", i),
            creator: format!("creator_{}", i),
            program: source.name().to_lowercase(),
            slot: 100000 + i as u64,
            deploy_timestamp: now - 1,
            detection_timestamp: now,
            source: *source,
            instruction_summary: None,
            is_jito_bundle: None,
            metadata: HashMap::new(),
        };

        detector.process_event(event).await.unwrap();
    }

    // Should receive all three candidates
    for i in 0..3 {
        let candidate = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("Timeout waiting for candidate")
            .expect("Channel closed");

        assert!(candidate.mint.starts_with("mint_"));
    }

    // Check metrics
    let metrics = detector.get_metrics().await;
    assert_eq!(metrics.total_events, 3);
    assert_eq!(metrics.forwarded_events, 3);
}

#[tokio::test]
async fn test_detection_metrics_latency_calculation() {
    let mut metrics = DetectionMetrics::new();

    // Add various latencies
    metrics.latencies_ms.push_back(100);
    metrics.latencies_ms.push_back(500);
    metrics.latencies_ms.push_back(800);
    metrics.latencies_ms.push_back(1200); // Over 1s
    metrics.latencies_ms.push_back(1500); // Over 1s

    metrics.total_events = 5;

    // P50 should be median (800 in this sorted list: 100, 500, 800, 1200, 1500)
    assert_eq!(metrics.p50_latency_ms(), Some(800));

    // P95 should be 95th percentile (1500)
    assert_eq!(metrics.p95_latency_ms(), Some(1500));

    // Detection accuracy should be 60% (3 out of 5 under 1s)
    let accuracy = metrics.detection_accuracy();
    assert!(accuracy >= 59.0 && accuracy <= 61.0);
}

#[tokio::test]
async fn test_detection_event_conversion() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let mut metadata = HashMap::new();
    metadata.insert("name".to_string(), "TestToken".to_string());
    metadata.insert("symbol".to_string(), "TEST".to_string());

    let event = DetectionEvent {
        mint: "test_mint".to_string(),
        creator: "test_creator".to_string(),
        program: "pump.fun".to_string(),
        slot: 12345,
        deploy_timestamp: now - 1,
        detection_timestamp: now,
        source: DetectionSource::PumpFun,
        instruction_summary: Some("create token".to_string()),
        is_jito_bundle: Some(true),
        metadata,
    };

    // Convert to PremintCandidate
    let candidate = event.to_premint_candidate();

    assert_eq!(candidate.mint, "test_mint");
    assert_eq!(candidate.creator, "test_creator");
    assert_eq!(candidate.program, "pump.fun");
    assert_eq!(candidate.slot, 12345);
    assert_eq!(candidate.timestamp, now - 1);
    assert_eq!(
        candidate.instruction_summary,
        Some("create token".to_string())
    );
    assert_eq!(candidate.is_jito_bundle, Some(true));
}

#[tokio::test]
async fn test_detection_sensitivity_filtering() {
    let (tx, mut rx) = mpsc::channel(100);
    let mut config = TokenDetectorConfig::default();

    // Set strict sensitivity - only PumpFun enabled
    config.enabled_sources = HashSet::new();
    config.enabled_sources.insert(DetectionSource::PumpFun);

    let detector = TokenDetector::new(config, tx);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Event from enabled source
    let event1 = DetectionEvent {
        mint: "mint_1".to_string(),
        creator: "creator_1".to_string(),
        program: "pump.fun".to_string(),
        slot: 100000,
        deploy_timestamp: now - 1,
        detection_timestamp: now,
        source: DetectionSource::PumpFun,
        instruction_summary: None,
        is_jito_bundle: None,
        metadata: HashMap::new(),
    };

    // Event from disabled source
    let event2 = DetectionEvent {
        mint: "mint_2".to_string(),
        creator: "creator_2".to_string(),
        program: "raydium".to_string(),
        slot: 100001,
        deploy_timestamp: now - 1,
        detection_timestamp: now,
        source: DetectionSource::Raydium, // Not enabled
        instruction_summary: None,
        is_jito_bundle: None,
        metadata: HashMap::new(),
    };

    detector.process_event(event1).await.unwrap();
    detector.process_event(event2).await.unwrap();

    // Should only receive first candidate
    let candidate = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("Timeout waiting for candidate")
        .expect("Channel closed");

    assert_eq!(candidate.mint, "mint_1");

    // Second should not be received (filtered by sensitivity)
    let result = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
    assert!(result.is_err(), "Should not receive filtered event");
}

#[tokio::test]
async fn test_detection_metrics_by_source() {
    let mut metrics = DetectionMetrics::new();

    // Record detections from different sources
    *metrics
        .detections_by_source
        .entry(DetectionSource::PumpFun)
        .or_insert(0) += 5;
    *metrics
        .detections_by_source
        .entry(DetectionSource::Raydium)
        .or_insert(0) += 3;
    *metrics
        .detections_by_source
        .entry(DetectionSource::Jupiter)
        .or_insert(0) += 2;

    assert_eq!(
        metrics.detection_rate_by_source(DetectionSource::PumpFun),
        5
    );
    assert_eq!(
        metrics.detection_rate_by_source(DetectionSource::Raydium),
        3
    );
    assert_eq!(
        metrics.detection_rate_by_source(DetectionSource::Jupiter),
        2
    );
    assert_eq!(
        metrics.detection_rate_by_source(DetectionSource::PumpSwap),
        0
    );
}

#[tokio::test]
async fn test_clock_skew_filtering() {
    let (tx, mut rx) = mpsc::channel(100);
    let mut config = TokenDetectorConfig::default();

    // Set max clock skew to 5 seconds
    config.sensitivity.max_clock_skew_secs = 5;

    let detector = TokenDetector::new(config, tx);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Event with acceptable clock skew
    let event1 = DetectionEvent {
        mint: "mint_1".to_string(),
        creator: "creator_1".to_string(),
        program: "pump.fun".to_string(),
        slot: 100000,
        deploy_timestamp: now - 2, // 2 seconds ago
        detection_timestamp: now,
        source: DetectionSource::PumpFun,
        instruction_summary: None,
        is_jito_bundle: None,
        metadata: HashMap::new(),
    };

    // Event with excessive clock skew
    let event2 = DetectionEvent {
        mint: "mint_2".to_string(),
        creator: "creator_2".to_string(),
        program: "pump.fun".to_string(),
        slot: 100001,
        deploy_timestamp: now - 10, // 10 seconds ago - exceeds limit
        detection_timestamp: now,
        source: DetectionSource::PumpFun,
        instruction_summary: None,
        is_jito_bundle: None,
        metadata: HashMap::new(),
    };

    detector.process_event(event1).await.unwrap();
    detector.process_event(event2).await.unwrap();

    // Should only receive first candidate
    let candidate = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("Timeout waiting for candidate")
        .expect("Channel closed");

    assert_eq!(candidate.mint, "mint_1");

    // Second should not be received (filtered by clock skew)
    let result = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
    assert!(result.is_err(), "Should not receive clock skew event");
}

#[tokio::test]
async fn test_detector_reset_metrics() {
    let (tx, _rx) = mpsc::channel(100);
    let config = TokenDetectorConfig::default();
    let detector = TokenDetector::new(config, tx);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let event = DetectionEvent {
        mint: "test_mint".to_string(),
        creator: "creator".to_string(),
        program: "pump.fun".to_string(),
        slot: 100000,
        deploy_timestamp: now - 1,
        detection_timestamp: now,
        source: DetectionSource::PumpFun,
        instruction_summary: None,
        is_jito_bundle: None,
        metadata: HashMap::new(),
    };

    // Process event
    detector.process_event(event).await.unwrap();

    // Verify metrics recorded
    let metrics = detector.get_metrics().await;
    assert_eq!(metrics.total_events, 1);

    // Reset metrics
    detector.reset_metrics().await;

    // Verify metrics cleared
    let metrics = detector.get_metrics().await;
    assert_eq!(metrics.total_events, 0);
    assert_eq!(metrics.forwarded_events, 0);
    assert_eq!(metrics.duplicate_count, 0);
}

#[test]
fn test_detection_source_names() {
    assert_eq!(DetectionSource::PumpFun.name(), "Pump.fun");
    assert_eq!(DetectionSource::Raydium.name(), "Raydium");
    assert_eq!(DetectionSource::Jupiter.name(), "Jupiter");
    assert_eq!(DetectionSource::PumpSwap.name(), "PumpSwap");
    assert_eq!(DetectionSource::Geyser.name(), "Geyser");
    assert_eq!(DetectionSource::WebSocket.name(), "WebSocket");
}

#[test]
fn test_detection_sensitivity_defaults() {
    let sensitivity = DetectionSensitivity::default();

    assert_eq!(sensitivity.min_confidence, 0.7);
    assert!(!sensitivity.enable_experimental_sources);
    assert_eq!(sensitivity.max_clock_skew_secs, 5);
    assert!(!sensitivity.require_multi_source);
    assert_eq!(sensitivity.min_confirmation_sources, 1);
}

#[test]
fn test_config_defaults() {
    let config = TokenDetectorConfig::default();

    assert!(config.enabled_sources.contains(&DetectionSource::PumpFun));
    assert!(config.enabled_sources.contains(&DetectionSource::Raydium));
    assert!(config.enabled_sources.contains(&DetectionSource::Jupiter));
    assert_eq!(config.dedup_window_ms, 500);
    assert_eq!(config.max_buffer_size, 1000);
    assert_eq!(config.metrics_window_secs, 3600);
}
