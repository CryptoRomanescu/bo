# Feature Engineering Framework for ML Models

## Overview

The Feature Engineering Framework provides a comprehensive, production-ready system for generating, managing, and utilizing 200+ features for machine learning models and heuristics in the H-5N1P3R trading oracle.

## Features

### Core Capabilities

- **200+ Features**: Organized into 10 categories covering all aspects of token analysis
- **Extensible API**: Easy to add new features through the `FeatureExtractor` trait
- **Feature Catalog**: Complete registry with metadata for all features
- **Normalization**: Multiple normalization methods (MinMax, Z-Score, Log, Sigmoid, Robust)
- **Feature Selection**: Importance ranking and selection methods
- **Feature Store**: Efficient caching layer to avoid redundant computation
- **Batch Processing**: Optimized for processing multiple tokens
- **Full Documentation**: Each feature is documented with description and expected range

## Feature Categories

### 1. Price Features (30+)
Features derived from token price data:
- Current price (SOL and USD)
- Price changes (1m, 5m, 15m, 1h, 24h)
- Price volatility and momentum
- Support/resistance levels
- Breakout and consolidation scores

### 2. Volume Features (30+)
Trading volume metrics:
- Volume at different time intervals
- Volume growth rates
- Buy/sell volume ratio
- Trade frequency and size distribution
- Volume spikes and consistency

### 3. Liquidity Features (20+)
Liquidity and market depth:
- SOL and token liquidity
- Liquidity changes over time
- Slippage estimates
- Pool depth and concentration
- Market depth quality

### 4. Holder Features (30+)
Token holder distribution:
- Holder count and growth
- Top holder concentration (top 10, 50, 100)
- Gini coefficient
- Whale analysis
- Creator holdings and behavior
- Holder churn and retention

### 5. Technical Indicators (30+)
Standard technical analysis indicators:
- RSI (multiple periods)
- MACD and signal lines
- EMA/SMA (multiple periods)
- Bollinger Bands
- Stochastic oscillators
- ADX, ATR, CCI, Williams %R
- Ichimoku, Parabolic SAR

### 6. Social Features (20+)
Social media and sentiment:
- Twitter mentions and sentiment
- Telegram and Discord metrics
- Social velocity and momentum
- Influencer mentions
- Community strength

### 7. On-Chain Features (20+)
Blockchain metrics:
- Transaction counts and growth
- Unique wallet activity
- Failed transaction rates
- Smart money flow
- MEV and Jito bundle activity
- Program interactions

### 8. Time-Series Features (20+)
Time-series analysis:
- Autocorrelation
- Seasonality patterns
- Trend strength
- Hurst exponent
- Fractal dimension
- Entropy rate
- Recurrence analysis

### 9. Interaction Features (20+)
Cross-feature combinations:
- Price-volume correlation
- Liquidity-volume efficiency
- Holder-price health
- Social-price lag analysis
- Composite health scores

### 10. Market Context (10+)
Broader market conditions:
- Market regime
- SOL price and volatility
- Overall market sentiment
- Network congestion
- Market correlation

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│         FeatureEngineeringPipeline                      │
│                                                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐ │
│  │   Catalog    │  │  Extractors  │  │  Normalizer  │ │
│  │   (200+      │  │  (Multiple   │  │  (Various    │ │
│  │   features)  │  │  categories) │  │  methods)    │ │
│  └──────────────┘  └──────────────┘  └──────────────┘ │
│                                                         │
│  ┌──────────────┐  ┌──────────────┐                   │
│  │   Selector   │  │ Feature Store│                   │
│  │  (Importance │  │  (Caching)   │                   │
│  │   ranking)   │  │              │                   │
│  └──────────────┘  └──────────────┘                   │
└─────────────────────────────────────────────────────────┘
```

## Usage

### Basic Usage

```rust
use h_5n1p3r::oracle::feature_engineering::{
    FeatureEngineeringPipeline,
    FeatureCategory,
    NormalizationMethod,
};

// Create pipeline
let pipeline = FeatureEngineeringPipeline::new();

// Extract features
let features = pipeline
    .extract_features(&candidate, &token_data)
    .await?;

// Extract and normalize
let normalized_features = pipeline
    .extract_and_normalize(&candidate, &token_data)
    .await?;
```

### Advanced Usage with Selection

```rust
use h_5n1p3r::oracle::feature_engineering::{
    FeatureEngineeringPipeline,
    SelectionMethod,
    MutualInformationSelector,
};

let mut pipeline = FeatureEngineeringPipeline::new();

// Train feature selector on historical data
let training_features: Vec<FeatureVector> = // ... collect training data
let targets: Vec<f64> = // ... target values (profit/loss)

pipeline.train_selector(&training_features, &targets)?;

// Extract features with automatic selection
let features = pipeline
    .extract_processed(
        &candidate,
        &token_data,
        Some(SelectionMethod::TopK(50)) // Select top 50 features
    )
    .await?;
```

### Custom Configuration

```rust
use std::time::Duration;

let pipeline = FeatureEngineeringPipeline::with_config(
    NormalizationMethod::ZScore,  // Use Z-score normalization
    10000,                          // Cache 10k entries
    Duration::from_secs(600),       // 10 minute TTL
);
```

### Accessing the Catalog

```rust
let catalog = pipeline.catalog();

// Get all features
println!("Total features: {}", catalog.feature_count());

// Get features by category
let price_features = catalog.features_by_category(FeatureCategory::Price);
for feature in price_features {
    println!("{}: {}", feature.id.name, feature.description);
}

// Get specific feature metadata
if let Some(feature) = catalog.get_feature("price_current") {
    println!("Description: {}", feature.description);
    println!("Range: {:?}", feature.value_range);
}
```

### Feature Selection

```rust
use h_5n1p3r::oracle::feature_engineering::{
    FeatureImportance,
    FeatureSelector,
    SelectionMethod,
    VarianceSelector,
    CorrelationSelector,
};

// Manual importance ranking
let mut importance = FeatureImportance::new();
importance.set_importance("price_momentum_short".to_string(), 0.95);
importance.set_importance("volume_spike_score".to_string(), 0.88);
importance.set_importance("liquidity_sol".to_string(), 0.82);

let selector = FeatureSelector::new(importance);

// Select top 20 features
let selected = selector.select(SelectionMethod::TopK(20));

// Select features above threshold
let selected = selector.select(SelectionMethod::Threshold(0.7));

// Select by percentile
let selected = selector.select(SelectionMethod::Percentile(90.0));
```

### Batch Processing

```rust
use h_5n1p3r::oracle::feature_engineering::BatchNormalizer;

let feature_vectors: Vec<FeatureVector> = // ... extract for multiple tokens

// Normalize entire batch
let normalized = BatchNormalizer::normalize_batch(
    &feature_vectors,
    NormalizationMethod::MinMax
)?;
```

## Extending the Framework

### Adding New Features

1. **Define the feature in the catalog:**

```rust
// In catalog.rs, add to appropriate register function
fn register_custom_features(&mut self) {
    let features = vec![
        ("my_custom_feature", "Description of my feature"),
    ];
    
    for (name, desc) in features {
        self.features.push(FeatureMetadata {
            id: FeatureId::new(name, FeatureCategory::Custom),
            description: desc.to_string(),
            value_range: (0.0, 1.0),
            requires_normalization: true,
            importance_rank: None,
            dependencies: vec![],
            computation_cost: ComputationCost::Medium,
        });
    }
}
```

2. **Implement the extractor:**

```rust
pub struct CustomFeatureExtractor;

impl FeatureExtractor for CustomFeatureExtractor {
    fn extract(
        &self,
        candidate: &PremintCandidate,
        token_data: &TokenData
    ) -> Result<FeatureVector> {
        let mut features = HashMap::new();
        
        // Your custom logic here
        let value = compute_custom_metric(token_data);
        features.insert("my_custom_feature".to_string(), value);
        
        Ok(features)
    }
    
    fn category(&self) -> FeatureCategory {
        FeatureCategory::Custom
    }
    
    fn feature_names(&self) -> Vec<String> {
        vec!["my_custom_feature".to_string()]
    }
}
```

3. **Add to the pipeline:**

```rust
// In extractors.rs, add to FeatureExtractionPipeline::new()
let extractors: Vec<Box<dyn FeatureExtractor>> = vec![
    // ... existing extractors
    Box::new(CustomFeatureExtractor),
];
```

## Performance Considerations

### Caching

The feature store automatically caches computed features with:
- Configurable TTL (default: 5 minutes)
- LRU eviction policy
- Version tracking for cache invalidation

### Computation Cost

Features are classified by computation cost:
- **Low**: Simple calculations (current values, ratios)
- **Medium**: Requires iteration (statistics, technical indicators)
- **High**: Complex analysis (time-series, external API calls)

For real-time applications, consider:
1. Pre-computing high-cost features
2. Using feature selection to reduce computation
3. Adjusting cache TTL based on update frequency

### Batch Processing

For processing multiple tokens:
```rust
use tokio::task::JoinSet;

let mut tasks = JoinSet::new();

for (candidate, token_data) in token_pairs {
    let pipeline_clone = Arc::clone(&pipeline);
    tasks.spawn(async move {
        pipeline_clone.extract_features(&candidate, &token_data).await
    });
}

while let Some(result) = tasks.join_next().await {
    // Process results
}
```

## Testing

Each module includes comprehensive unit tests:

```bash
# Run all feature engineering tests
cargo test feature_engineering

# Run specific module tests
cargo test feature_engineering::catalog
cargo test feature_engineering::extractors
cargo test feature_engineering::normalization
cargo test feature_engineering::selection
cargo test feature_engineering::store
```

## Feature Importance

The framework includes several methods for ranking feature importance:

1. **Mutual Information**: Measures statistical dependence between feature and target
2. **Correlation**: Linear relationship with target variable
3. **Variance**: Features with low variance are less informative
4. **Model-based**: Use ML model feature importance (integrate with your models)

## Integration with Existing Oracle

The feature engineering framework integrates seamlessly with the existing `PredictiveOracle`:

```rust
// In quantum_oracle.rs
use crate::oracle::feature_engineering::FeatureEngineeringPipeline;

pub struct PredictiveOracle {
    feature_pipeline: FeatureEngineeringPipeline,
    // ... other fields
}

impl PredictiveOracle {
    pub async fn analyze(&self, candidate: &PremintCandidate) -> Result<ScoredCandidate> {
        let token_data = self.fetch_token_data(candidate).await?;
        
        // Extract features
        let features = self.feature_pipeline
            .extract_and_normalize(candidate, &token_data)
            .await?;
        
        // Use features in scoring
        let score = self.compute_score(&features)?;
        
        // ... rest of analysis
    }
}
```

## Best Practices

1. **Feature Selection**: Always use feature selection for production models to reduce overfitting
2. **Normalization**: Fit normalizer on training data, then apply to test/production data
3. **Caching**: Monitor cache hit rates and adjust TTL/capacity accordingly
4. **Documentation**: Document any custom features you add
5. **Versioning**: Use feature versioning to track changes and invalidate old cached features
6. **Testing**: Write unit tests for all custom features

## Troubleshooting

### Cache Misses
- Check TTL configuration
- Verify cache capacity is sufficient
- Monitor memory usage

### Feature Extraction Errors
- Verify TokenData has all required fields
- Check for missing data handling in extractors
- Review error logs for specific issues

### Poor Feature Importance
- Ensure sufficient training data
- Check for data quality issues
- Consider feature engineering improvements
- Remove highly correlated features

## Future Enhancements

Potential improvements for future versions:

- [ ] Real-time feature streaming
- [ ] Distributed feature computation
- [ ] GPU-accelerated technical indicators
- [ ] AutoML feature engineering
- [ ] Feature store persistence to database
- [ ] Feature monitoring and drift detection
- [ ] A/B testing framework for features
- [ ] Integration with MLflow for experiment tracking

## References

- Token Analysis: See `src/oracle/quantum_oracle.rs`
- Type Definitions: See `src/oracle/types.rs`
- Pattern Recognition: See `src/oracle/pattern_memory.rs`
- Performance Monitoring: See `src/oracle/performance_monitor.rs`

## Support

For questions or issues with the feature engineering framework:
1. Check the inline documentation in source files
2. Review unit tests for usage examples
3. Consult the main README.md for system architecture
4. Open an issue on GitHub with [Feature Engineering] prefix
