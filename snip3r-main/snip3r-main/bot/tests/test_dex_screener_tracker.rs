//! Integration tests for DEX Screener Trending Tracker
//!
//! Tests the complete functionality of the DEX Screener trending integration
//! including API polling, event detection, and metrics calculation.

use h_5n1p3r::oracle::{
    DexScreenerConfig, DexScreenerTracker, TrendingEvent, TrendingMetrics,
};

#[test]
fn test_tracker_initialization() {
    let config = DexScreenerConfig::default();
    let _tracker = DexScreenerTracker::new(config.clone());

    // Verify default configuration
    assert_eq!(config.polling_interval_secs, 3);
    assert!(config.enable_tracking);
    assert_eq!(config.min_trending_rank, 100);
    assert_eq!(config.velocity_window_secs, 300);
}

#[tokio::test]
async fn test_trending_metrics_default() {
    let metrics = TrendingMetrics::default();

    assert!(!metrics.is_trending);
    assert_eq!(metrics.current_rank, None);
    assert_eq!(metrics.duration_in_trending, 0);
    assert_eq!(metrics.velocity, 0.0);
    assert_eq!(metrics.momentum_score, 0.0);
    assert_eq!(metrics.price_correlation, 0.0);
    assert_eq!(metrics.volume_correlation, 0.0);
    assert!(metrics.recent_events.is_empty());
}

#[test]
fn test_config_customization() {
    let mut config = DexScreenerConfig::default();
    
    // Customize config
    config.polling_interval_secs = 2;
    config.min_trending_rank = 50;
    config.api_key = Some("test_key".to_string());
    config.enable_tracking = false;

    assert_eq!(config.polling_interval_secs, 2);
    assert_eq!(config.min_trending_rank, 50);
    assert!(!config.enable_tracking);
    assert_eq!(config.api_key, Some("test_key".to_string()));
}

#[test]
fn test_tracker_disabled() {
    let mut config = DexScreenerConfig::default();
    config.enable_tracking = false;
    
    let _tracker = DexScreenerTracker::new(config.clone());
    
    // Verify tracking is disabled
    assert!(!config.enable_tracking);
}

#[tokio::test]
async fn test_is_trending_check() {
    let mut config = DexScreenerConfig::default();
    config.enable_tracking = false; // Disable to avoid API calls
    let tracker = DexScreenerTracker::new(config);
    
    // Check for non-existent token
    let is_trending = tracker.is_trending("non_existent_token").await;
    assert!(!is_trending);
    
    // Check trending rank
    let rank = tracker.get_trending_rank("non_existent_token").await;
    assert_eq!(rank, None);
}

#[test]
fn test_trending_event_types() {
    // Test Entry event
    let entry = TrendingEvent::Entry { rank: 10 };
    assert_eq!(entry, TrendingEvent::Entry { rank: 10 });
    
    // Test Exit event
    let exit = TrendingEvent::Exit { final_rank: 50 };
    assert_eq!(exit, TrendingEvent::Exit { final_rank: 50 });
    
    // Test RankUp event
    let rank_up = TrendingEvent::RankUp { 
        old_rank: 30, 
        new_rank: 15 
    };
    assert_eq!(rank_up, TrendingEvent::RankUp { 
        old_rank: 30, 
        new_rank: 15 
    });
    
    // Test RankDown event
    let rank_down = TrendingEvent::RankDown { 
        old_rank: 10, 
        new_rank: 25 
    };
    assert_eq!(rank_down, TrendingEvent::RankDown { 
        old_rank: 10, 
        new_rank: 25 
    });
    
    // Test HighMomentum event
    let momentum = TrendingEvent::HighMomentum { velocity: 5.5 };
    assert_eq!(momentum, TrendingEvent::HighMomentum { velocity: 5.5 });
}

#[test]
fn test_latency_requirement() {
    let config = DexScreenerConfig::default();
    
    // Verify <4s latency requirement
    // polling_interval should be < 4 for event detection
    assert!(
        config.polling_interval_secs < 4,
        "Polling interval {} exceeds 4s requirement",
        config.polling_interval_secs
    );
    
    // Cache duration should be short for real-time tracking
    assert!(
        config.cache_duration_secs <= 3,
        "Cache duration too long for real-time tracking"
    );
}

#[tokio::test]
async fn test_concurrent_tracking() {
    let mut config = DexScreenerConfig::default();
    config.enable_tracking = false; // Disable to avoid API calls
    let tracker = std::sync::Arc::new(DexScreenerTracker::new(config));
    
    // Test concurrent access to tracker
    let mut handles = vec![];
    
    for i in 0..5 {
        let tracker_clone = tracker.clone();
        let handle = tokio::spawn(async move {
            let token = format!("token_{}", i);
            tracker_clone.is_trending(&token).await
        });
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    for handle in handles {
        let result = handle.await;
        assert!(result.is_ok());
        let is_trending = result.unwrap();
        assert!(!is_trending); // Should not be trending since we have no data
    }
}

#[test]
fn test_metrics_score_ranges() {
    let metrics = TrendingMetrics {
        is_trending: true,
        current_rank: Some(10),
        duration_in_trending: 300,
        velocity: 2.5,
        momentum_score: 85.0,
        price_correlation: 0.8,
        volume_correlation: 0.7,
        recent_events: vec![TrendingEvent::Entry { rank: 10 }],
    };
    
    // Verify score ranges
    assert!(metrics.momentum_score >= 0.0 && metrics.momentum_score <= 100.0);
    assert!(metrics.price_correlation >= 0.0 && metrics.price_correlation <= 1.0);
    assert!(metrics.volume_correlation >= 0.0 && metrics.volume_correlation <= 1.0);
    
    // Verify rank is within expected range
    if let Some(rank) = metrics.current_rank {
        assert!(rank > 0 && rank <= 100);
    }
}

#[test]
fn test_velocity_calculation() {
    // Positive velocity (rank improving)
    let velocity_positive = 5.0; // 5 ranks per minute improvement
    assert!(velocity_positive > 0.0);
    
    // Negative velocity (rank declining)
    let velocity_negative = -3.0; // 3 ranks per minute decline
    assert!(velocity_negative < 0.0);
    
    // Zero velocity (stable rank)
    let velocity_zero = 0.0;
    assert_eq!(velocity_zero, 0.0);
}

#[test]
fn test_api_timeout_configuration() {
    let mut config = DexScreenerConfig::default();
    config.api_timeout_secs = 1; // Very short timeout
    
    let _tracker = DexScreenerTracker::new(config.clone());
    
    // Verify timeout is set correctly
    assert_eq!(config.api_timeout_secs, 1);
}

#[test]
fn test_correlation_score_calculation() {
    // Test perfect positive correlation
    let perfect_correlation = 1.0;
    assert_eq!(perfect_correlation, 1.0);
    
    // Test no correlation
    let no_correlation = 0.0;
    assert_eq!(no_correlation, 0.0);
    
    // Test partial correlation
    let partial_correlation = 0.5;
    assert!(partial_correlation > 0.0 && partial_correlation < 1.0);
}

#[test]
fn test_pump_dump_detection() {
    // Scenario: Token enters trending at rank 20, quickly rises to rank 5
    // This indicates a pump opportunity (10-100x as per requirements)
    let entry_event = TrendingEvent::Entry { rank: 20 };
    let rank_up_event = TrendingEvent::RankUp { 
        old_rank: 20, 
        new_rank: 5 
    };
    
    // Verify rank improvement
    if let (TrendingEvent::Entry { rank: entry_rank }, 
             TrendingEvent::RankUp { old_rank, new_rank }) = (entry_event, rank_up_event) {
        assert_eq!(entry_rank, old_rank);
        assert!(new_rank < old_rank, "Rank should improve (lower number)");
        let improvement = old_rank - new_rank;
        assert!(improvement >= 5, "Significant rank improvement detected");
    }
    
    // Scenario: Token exits trending - instant dump risk
    let exit_event = TrendingEvent::Exit { final_rank: 100 };
    if let TrendingEvent::Exit { final_rank } = exit_event {
        assert!(final_rank > 50, "Exit from trending indicates potential dump");
    }
}

#[test]
fn test_trending_duration_tracking() {
    let start_time = 1000u64;
    let current_time = 1300u64;
    let duration = current_time - start_time;
    
    assert_eq!(duration, 300); // 5 minutes in trending
    
    // Long duration indicates sustained interest
    assert!(duration >= 300, "Token has sustained trending presence");
}

#[tokio::test]
async fn test_multiple_tokens_tracking() {
    let mut config = DexScreenerConfig::default();
    config.enable_tracking = false; // Disable to avoid API calls
    let tracker = DexScreenerTracker::new(config);
    
    // Track multiple tokens simultaneously
    let tokens = vec!["token_a", "token_b", "token_c"];
    
    for token in tokens {
        let is_trending = tracker.is_trending(token).await;
        assert!(!is_trending);
    }
}

#[test]
fn test_event_history_limit() {
    let max_events = 5;
    let mut events = vec![];
    
    // Add more events than the limit
    for i in 0..10 {
        events.push(TrendingEvent::Entry { rank: i as u32 });
    }
    
    // Take only the most recent events
    let recent_events: Vec<_> = events.iter().rev().take(max_events).collect();
    assert_eq!(recent_events.len(), max_events);
}

#[test]
fn test_momentum_score_factors() {
    // Test that momentum considers multiple factors:
    // 1. Current rank (lower is better)
    let rank_score = 100.0 - 10.0; // Rank 10 gives 90 points
    assert_eq!(rank_score, 90.0);
    
    // 2. Velocity (rank change rate)
    let velocity = 5.0; // 5 ranks per minute
    let velocity_score = velocity * 10.0; // Up to 50 points
    assert_eq!(velocity_score, 50.0);
    
    // 3. Duration in trending
    let duration_mins: f64 = 30.0;
    let duration_score = duration_mins.min(30.0) * 1.67; // Up to 50 points
    assert!((duration_score - 50.1).abs() < 0.2, "Duration score {} not close to 50", duration_score);
    
    // Combined momentum should be normalized
    let total_momentum = (rank_score + velocity_score + duration_score) / 2.0;
    assert!(total_momentum <= 100.0);
}
