# Pattern Memory Integration Guide

This guide shows how to integrate the Temporal Pattern Memory system with the existing Oracle/QuantumOracle.

## Quick Integration

### Step 1: Add Pattern Memory to Oracle State

```rust
use h_5n1p3r::oracle::{PatternMemory, Observation};

pub struct PredictiveOracle {
    // ... existing fields ...
    
    /// Temporal pattern memory for learning from historical outcomes
    pattern_memory: Arc<Mutex<PatternMemory>>,
}
```

### Step 2: Initialize in Constructor

```rust
impl PredictiveOracle {
    pub fn new(config: OracleConfig, storage: Arc<dyn LedgerStorage>) -> Self {
        Self {
            // ... existing initialization ...
            
            pattern_memory: Arc::new(Mutex::new(PatternMemory::with_capacity(1000))),
        }
    }
}
```

### Step 3: Extract Features Helper

Add a method to convert scoring data into observations:

```rust
impl PredictiveOracle {
    /// Converts current and historical candidate data into pattern observations
    fn extract_pattern_observations(
        &self,
        candidate: &PremintCandidate,
        historical_data: Option<&[PremintCandidate]>,
    ) -> Vec<Observation> {
        let mut observations = Vec::new();
        
        // Create observation from current state
        let current_obs = Observation {
            features: vec![
                self.normalize_liquidity(candidate.liquidity_sol),
                self.normalize_volume_growth(candidate.volume_growth_24h),
                self.normalize_holder_count(candidate.holder_count),
                self.normalize_price_change(candidate.price_change_1h),
                if candidate.jito_bundle_included { 1.0 } else { 0.0 },
            ],
            timestamp: candidate.timestamp,
        };
        observations.push(current_obs);
        
        // Add historical observations if available
        if let Some(history) = historical_data {
            for hist in history.iter().take(5) {  // Use last 5 historical points
                let hist_obs = Observation {
                    features: vec![
                        self.normalize_liquidity(hist.liquidity_sol),
                        self.normalize_volume_growth(hist.volume_growth_24h),
                        self.normalize_holder_count(hist.holder_count),
                        self.normalize_price_change(hist.price_change_1h),
                        if hist.jito_bundle_included { 1.0 } else { 0.0 },
                    ],
                    timestamp: hist.timestamp,
                };
                observations.push(hist_obs);
            }
        }
        
        observations
    }
    
    // Normalization helpers (0.0-1.0 range)
    fn normalize_liquidity(&self, liquidity: f64) -> f32 {
        (liquidity / 100.0).min(1.0).max(0.0) as f32
    }
    
    fn normalize_volume_growth(&self, growth: f64) -> f32 {
        ((growth / 10.0) + 0.5).min(1.0).max(0.0) as f32
    }
    
    fn normalize_holder_count(&self, count: u32) -> f32 {
        (count as f64 / 1000.0).min(1.0).max(0.0) as f32
    }
    
    fn normalize_price_change(&self, change: f64) -> f32 {
        ((change / 2.0) + 0.5).min(1.0).max(0.0) as f32
    }
}
```

### Step 4: Use Pattern Memory in Scoring

```rust
impl PredictiveOracle {
    pub async fn score_candidate(
        &self,
        candidate: PremintCandidate,
        historical_data: Option<Vec<PremintCandidate>>,
    ) -> Result<ScoredCandidate> {
        // ... existing scoring logic ...
        
        let mut base_score = self.calculate_base_score(&candidate).await?;
        
        // Extract pattern observations
        let observations = self.extract_pattern_observations(
            &candidate,
            historical_data.as_deref(),
        );
        
        // Check for matching patterns (only if we have enough observations)
        if observations.len() >= 2 {
            let pattern_memory = self.pattern_memory.lock().await;
            
            if let Some(pattern_match) = pattern_memory.find_match(&observations) {
                let confidence = pattern_match.pattern.confidence;
                let predicted_pnl = pattern_match.predicted_pnl;
                
                // Adjust score based on pattern prediction
                if confidence > 0.7 {
                    if pattern_match.predicted_outcome {
                        // Historical pattern suggests success
                        let boost = (confidence * 20.0) as u8;  // Up to +20 points
                        base_score = base_score.saturating_add(boost);
                        
                        tracing::info!(
                            "Pattern boost: +{} points (confidence: {:.2}, predicted PnL: {:.3})",
                            boost, confidence, predicted_pnl
                        );
                    } else {
                        // Historical pattern suggests failure
                        let penalty = (confidence * 30.0) as u8;  // Up to -30 points
                        base_score = base_score.saturating_sub(penalty);
                        
                        tracing::warn!(
                            "Pattern penalty: -{} points (confidence: {:.2}, predicted PnL: {:.3})",
                            penalty, confidence, predicted_pnl
                        );
                    }
                }
                
                // Add to reason
                feature_scores.insert(
                    "pattern_match".to_string(),
                    confidence as f64,
                );
            }
        }
        
        // ... rest of scoring logic ...
    }
}
```

### Step 5: Record Outcomes

After a trade completes, record the pattern with outcome:

```rust
impl PredictiveOracle {
    pub async fn record_trade_outcome(
        &self,
        candidate: &PremintCandidate,
        historical_data: Option<&[PremintCandidate]>,
        outcome: bool,  // true for profit, false for loss
        pnl_sol: f32,
    ) {
        // Extract the same observations used for scoring
        let observations = self.extract_pattern_observations(
            candidate,
            historical_data,
        );
        
        if observations.len() >= 2 {
            let mut pattern_memory = self.pattern_memory.lock().await;
            pattern_memory.add_pattern(observations, outcome, pnl_sol);
            
            // Log statistics
            let stats = pattern_memory.stats();
            tracing::info!(
                "Pattern memory updated: {} patterns, {:.1}% success rate, avg PnL: {:.3} SOL",
                stats.total_patterns,
                stats.success_rate * 100.0,
                stats.avg_pnl
            );
        }
    }
}
```

### Step 6: Wire Up to Transaction Monitor

In the transaction monitoring flow, after determining outcome:

```rust
// In TransactionMonitor or wherever outcomes are determined
let outcome_successful = trade_result.profit > 0.0;
let pnl = trade_result.profit as f32;

// Record the pattern outcome
oracle.record_trade_outcome(
    &original_candidate,
    Some(&historical_data),
    outcome_successful,
    pnl,
).await;
```

## Alternative: Lightweight Integration

If you don't have historical data readily available, you can use a simpler single-observation pattern:

```rust
// Simple integration with just current state
fn create_simple_observation(&self, candidate: &PremintCandidate) -> Observation {
    Observation {
        features: vec![
            self.normalize_liquidity(candidate.liquidity_sol),
            self.normalize_volume_growth(candidate.volume_growth_24h),
            self.normalize_holder_count(candidate.holder_count),
            self.normalize_price_change(candidate.price_change_1h),
            self.normalize_market_cap(candidate.market_cap),
        ],
        timestamp: candidate.timestamp,
    }
}

// Use with 2 observations minimum (current + previous)
let obs1 = self.create_simple_observation(&previous_candidate);
let obs2 = self.create_simple_observation(&current_candidate);
pattern_memory.add_pattern(vec![obs1, obs2], outcome, pnl);
```

## Monitoring and Tuning

### Get Statistics

```rust
let stats = pattern_memory.lock().await.stats();
println!("Pattern Memory Stats:");
println!("  Total patterns: {}", stats.total_patterns);
println!("  Success rate: {:.1}%", stats.success_rate * 100.0);
println!("  Avg confidence: {:.2}", stats.avg_confidence);
println!("  Avg PnL: {:.3} SOL", stats.avg_pnl);
println!("  Memory usage: {:.2} KB", stats.memory_usage_bytes as f32 / 1024.0);
```

### Adjust Thresholds

Based on performance, you can tune the thresholds:

```rust
let mut pattern_memory = self.pattern_memory.lock().await;

// Require higher similarity for pattern matching (less aggressive merging)
pattern_memory.set_similarity_threshold(0.90);

// Require higher confidence for predictions
pattern_memory.set_confidence_threshold(0.75);
```

## Testing Integration

Create a test that validates the integration:

```rust
#[tokio::test]
async fn test_pattern_memory_integration() {
    let config = OracleConfig::default();
    let storage = Arc::new(InMemoryStorage::new());
    let oracle = PredictiveOracle::new(config, storage);
    
    // Create test candidates
    let candidate1 = create_test_candidate(/* high liquidity, growth */);
    let candidate2 = create_test_candidate(/* similar to candidate1 */);
    
    // Score without pattern
    let score1 = oracle.score_candidate(candidate1.clone(), None).await.unwrap();
    println!("Score without pattern: {}", score1.predicted_score);
    
    // Record successful outcome
    oracle.record_trade_outcome(&candidate1, None, true, 0.5).await;
    
    // Score similar candidate - should get boost
    let score2 = oracle.score_candidate(candidate2, None).await.unwrap();
    println!("Score with pattern: {}", score2.predicted_score);
    
    assert!(score2.predicted_score >= score1.predicted_score, 
            "Similar successful pattern should boost score");
}
```

## Performance Considerations

1. **Lock Contention**: Pattern memory uses `Mutex`, so lock it briefly
   ```rust
   let pattern_match = {
       let memory = self.pattern_memory.lock().await;
       memory.find_match(&observations)
   }; // Lock released here
   ```

2. **Batch Updates**: If recording many outcomes, consider batching:
   ```rust
   let mut memory = self.pattern_memory.lock().await;
   for (obs, outcome, pnl) in outcomes {
       memory.add_pattern(obs, outcome, pnl);
   }
   // Lock released after all updates
   ```

3. **Memory Monitoring**: Periodically check memory usage:
   ```rust
   if stats.memory_usage_bytes > 40_000 {  // 40KB threshold
       tracing::warn!("Pattern memory usage high: {} KB", 
                      stats.memory_usage_bytes / 1024);
   }
   ```

## Next Steps

1. Implement the feature extraction helpers
2. Wire up to your scoring function
3. Connect to transaction outcome tracking
4. Monitor statistics in production
5. Tune thresholds based on performance
6. Consider adding persistence (future enhancement)
