//! Basic test for the normalized storage implementation

use h_5n1p3r::oracle::{
    LedgerStorage, Outcome, ScoredCandidate, SqliteLedgerNormalized, TransactionRecord,
};
use h_5n1p3r::types::PremintCandidate;
use rand;
use std::collections::HashMap;

#[tokio::test]
async fn test_normalized_storage_basic() {
    // Create normalized storage
    let storage = SqliteLedgerNormalized::new()
        .await
        .expect("Failed to create normalized storage");

    // Create test data
    let mut feature_scores = HashMap::new();
    feature_scores.insert("liquidity".to_string(), 0.8);
    feature_scores.insert("holder_distribution".to_string(), 0.6);

    let mut market_context = HashMap::new();
    market_context.insert("total_volume".to_string(), 1000.0);
    market_context.insert("price_trend".to_string(), 0.5);

    let scored_candidate = ScoredCandidate {
        base: PremintCandidate {
            mint: "test_mint_123".to_string(),
            creator: "test_creator".to_string(),
            program: "pump.fun".to_string(),
            slot: 12345,
            timestamp: 1000000,
            instruction_summary: Some("Test instruction".to_string()),
            is_jito_bundle: Some(true),
        },
        mint: "test_mint_123".to_string(),
        predicted_score: 85,
        reason: "Test reason".to_string(),
        feature_scores,
        calculation_time: 1234,
        anomaly_detected: false,
        timestamp: 1000000,
    };

    let signature = format!("test_signature_{}", rand::random::<u64>());

    let record = TransactionRecord {
        id: None,
        scored_candidate,
        transaction_signature: Some(signature.clone()),
        buy_price_sol: Some(0.001),
        sell_price_sol: Some(0.0011),
        amount_bought_tokens: Some(1000.0),
        amount_sold_tokens: Some(1000.0),
        initial_sol_spent: Some(1.0),
        final_sol_received: Some(1.1),
        timestamp_decision_made: 1000000,
        timestamp_transaction_sent: Some(1000001),
        timestamp_outcome_evaluated: Some(1000002),
        actual_outcome: Outcome::Profit(0.1),
        market_context_snapshot: market_context,
    };

    // Test insert
    let id = storage
        .insert_record(&record)
        .await
        .expect("Failed to insert record");
    assert!(id > 0);

    // Test retrieval
    let records = storage
        .get_records_since(0)
        .await
        .expect("Failed to get records");
    let our_record = records
        .iter()
        .find(|r| r.scored_candidate.mint == "test_mint_123")
        .expect("Our record not found");

    assert_eq!(our_record.scored_candidate.mint, "test_mint_123");
    assert_eq!(our_record.scored_candidate.feature_scores.len(), 2);
    assert_eq!(our_record.market_context_snapshot.len(), 2);

    // Test update outcome
    storage
        .update_outcome(
            &signature,
            Outcome::Profit(0.2),
            None,
            None,
            None,
            Some(1.2),
            Some(1000003),
            true, // Mark as verified for the test
        )
        .await
        .expect("Failed to update outcome");

    // Test record count (should be at least 1, might have other records from other tests)
    let count = storage
        .get_record_count()
        .await
        .expect("Failed to get count");
    assert!(count >= 1);

    // Test health check
    let healthy = storage.health_check().await.expect("Health check failed");
    assert!(healthy);

    println!("Normalized storage test completed successfully!");
}
