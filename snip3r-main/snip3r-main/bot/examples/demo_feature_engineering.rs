//! Example: Using the Feature Engineering Framework
//!
//! This example demonstrates how to use the ML Feature Engineering Framework
//! to extract, normalize, and select features for token analysis.

use h_5n1p3r::oracle::feature_engineering::{
    FeatureCategory, FeatureEngineeringPipeline, NormalizationMethod, SelectionMethod,
};
use h_5n1p3r::oracle::types_old::{
    CreatorHoldings, HolderData, LiquidityPool, Metadata, PoolType, SocialActivity, TokenData,
    VolumeData,
};
use h_5n1p3r::types::PremintCandidate;
use std::collections::VecDeque;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("=== Feature Engineering Framework Demo ===\n");

    // 1. Create the feature engineering pipeline
    println!("1. Creating feature engineering pipeline...");
    let pipeline = FeatureEngineeringPipeline::with_config(
        NormalizationMethod::MinMax,
        1000,
        Duration::from_secs(300),
    );

    // 2. Display catalog information
    println!("2. Feature catalog information:");
    let catalog = pipeline.catalog();
    println!("   Total features: {}", catalog.feature_count());
    println!(
        "   Price features: {}",
        catalog.feature_count_by_category(FeatureCategory::Price)
    );
    println!(
        "   Volume features: {}",
        catalog.feature_count_by_category(FeatureCategory::Volume)
    );
    println!(
        "   Liquidity features: {}",
        catalog.feature_count_by_category(FeatureCategory::Liquidity)
    );
    println!(
        "   Holder features: {}",
        catalog.feature_count_by_category(FeatureCategory::Holders)
    );
    println!(
        "   Technical features: {}",
        catalog.feature_count_by_category(FeatureCategory::Technical)
    );
    println!();

    // 3. Create sample token data
    println!("3. Creating sample token data...");
    let candidate = PremintCandidate {
        mint: "ExampleMintAddress12345".to_string(),
        creator: "ExampleCreator12345".to_string(),
        program: "pump.fun".to_string(),
        slot: 12345678,
        timestamp: 1640995200,
        instruction_summary: Some("Token creation".to_string()),
        is_jito_bundle: Some(true),
    };

    let token_data = create_sample_token_data();
    println!("   Token: {}", candidate.mint);
    println!(
        "   Liquidity: {:.2} SOL",
        token_data.liquidity_pool.as_ref().unwrap().sol_amount
    );
    println!(
        "   Volume: {:.2} SOL",
        token_data.volume_data.current_volume
    );
    println!();

    // 4. Extract features
    println!("4. Extracting features...");
    let features = pipeline.extract_features(&candidate, &token_data).await?;
    println!("   Extracted {} features", features.len());

    // Display some example features
    println!("\n   Sample features:");
    if let Some(&price) = features.get("price_current") {
        println!("     - price_current: {:.6}", price);
    }
    if let Some(&volume) = features.get("volume_24h") {
        println!("     - volume_24h: {:.2}", volume);
    }
    if let Some(&liquidity) = features.get("liquidity_sol") {
        println!("     - liquidity_sol: {:.2}", liquidity);
    }
    if let Some(&holders) = features.get("holder_count") {
        println!("     - holder_count: {:.0}", holders);
    }
    println!();

    // 5. Extract and normalize features
    println!("5. Extracting and normalizing features...");
    let normalized_features = pipeline
        .extract_and_normalize(&candidate, &token_data)
        .await?;
    println!("   Normalized {} features", normalized_features.len());

    println!("\n   Sample normalized features (0-1 range):");
    if let Some(&price) = normalized_features.get("price_current") {
        println!("     - price_current: {:.4}", price);
    }
    if let Some(&volume) = normalized_features.get("volume_24h") {
        println!("     - volume_24h: {:.4}", volume);
    }
    println!();

    // 6. Feature extraction with caching
    println!("6. Testing feature caching...");
    let start = std::time::Instant::now();
    let _ = pipeline.extract_features(&candidate, &token_data).await?;
    let first_time = start.elapsed();

    let start = std::time::Instant::now();
    let _ = pipeline.extract_features(&candidate, &token_data).await?;
    let cached_time = start.elapsed();

    println!("   First extraction: {:?}", first_time);
    println!("   Cached extraction: {:?}", cached_time);
    println!(
        "   Speedup: {:.2}x",
        first_time.as_micros() as f64 / cached_time.as_micros().max(1) as f64
    );
    println!();

    // 7. Display pipeline stats
    println!("7. Pipeline statistics:");
    let stats = pipeline.get_stats().await;
    println!("   Total features: {}", stats.total_features);
    println!("   Available extractors: {}", stats.available_extractors);
    println!("   Cache entries: {}", stats.cache_entries);
    println!();

    // 8. Example feature metadata
    println!("8. Feature metadata example:");
    if let Some(feature) = catalog.get_feature("price_momentum_short") {
        println!("   Feature: {}", feature.id.name);
        println!("   Category: {:?}", feature.id.category);
        println!("   Description: {}", feature.description);
        println!("   Computation cost: {:?}", feature.computation_cost);
    }
    println!();

    println!("=== Demo Complete ===");
    println!("\nThe Feature Engineering Framework provides:");
    println!("  ✓ 200+ features across 10 categories");
    println!("  ✓ Multiple normalization methods");
    println!("  ✓ Feature importance ranking and selection");
    println!("  ✓ Efficient caching layer");
    println!("  ✓ Extensible API for custom features");
    println!("  ✓ Comprehensive documentation");

    Ok(())
}

fn create_sample_token_data() -> TokenData {
    TokenData {
        supply: 1_000_000_000,
        decimals: 9,
        metadata_uri: "https://example.com/metadata.json".to_string(),
        metadata: Some(Metadata {
            name: "Example Token".to_string(),
            symbol: "EXPL".to_string(),
            description: "An example token for demonstration".to_string(),
            image: "https://example.com/image.png".to_string(),
            attributes: vec![],
        }),
        holder_distribution: vec![
            HolderData {
                address: "Holder1".to_string(),
                percentage: 0.15,
                is_whale: false,
            },
            HolderData {
                address: "Holder2".to_string(),
                percentage: 0.10,
                is_whale: false,
            },
            HolderData {
                address: "Holder3".to_string(),
                percentage: 0.08,
                is_whale: false,
            },
        ],
        liquidity_pool: Some(LiquidityPool {
            sol_amount: 75.0,
            token_amount: 5000.0,
            pool_address: "PoolAddress123".to_string(),
            pool_type: PoolType::PumpFun,
        }),
        volume_data: VolumeData {
            initial_volume: 50.0,
            current_volume: 450.0,
            volume_growth_rate: 9.0,
            transaction_count: 150,
            buy_sell_ratio: 2.5,
        },
        creator_holdings: CreatorHoldings {
            initial_balance: 200_000_000,
            current_balance: 180_000_000,
            first_sell_timestamp: Some(1640995500),
            sell_transactions: 3,
        },
        holder_history: {
            let mut hist = VecDeque::new();
            hist.push_back(10);
            hist.push_back(25);
            hist.push_back(50);
            hist.push_back(100);
            hist
        },
        price_history: {
            let mut hist = VecDeque::new();
            hist.push_back(0.001);
            hist.push_back(0.0012);
            hist.push_back(0.0015);
            hist.push_back(0.0018);
            hist
        },
        social_activity: SocialActivity {
            twitter_mentions: 125,
            telegram_members: 450,
            discord_members: 300,
            social_score: 0.82,
        },
    }
}
