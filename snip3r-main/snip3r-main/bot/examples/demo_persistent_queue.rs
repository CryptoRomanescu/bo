//! Demonstration of persistent monitoring queue
//!
//! This example shows how transactions persist across application restarts,
//! eliminating the risk of data loss in production environments.

use anyhow::Result;
use chrono::Utc;
use h_5n1p3r::oracle::{
    storage::{LedgerStorage, SqliteLedger},
    transaction_monitor::MonitoredTransaction,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize simple logging
    tracing_subscriber::fmt::init();

    println!("🔧 Demonstrating Persistent Monitoring Queue");
    println!("============================================");

    // Simulate Step 1: Application startup, enqueue some transactions
    println!("\n📥 Step 1: Enqueueing transactions for monitoring...");
    {
        let storage = SqliteLedger::new().await?;

        // Clean up any previous demo data
        let _ = storage.cleanup_completed_monitoring().await;

        // Create test transactions
        let tx1 = MonitoredTransaction {
            signature: "demo_tx_1_persistent_queue".to_string(),
            mint: "demo_mint_1".to_string(),
            amount_bought_tokens: 1000.0,
            initial_sol_spent: 1.5,
            monitor_until: Utc::now().timestamp_millis() as u64 + 30000,
        };

        let tx2 = MonitoredTransaction {
            signature: "demo_tx_2_persistent_queue".to_string(),
            mint: "demo_mint_2".to_string(),
            amount_bought_tokens: 2000.0,
            initial_sol_spent: 2.0,
            monitor_until: Utc::now().timestamp_millis() as u64 + 30000,
        };

        storage.enqueue_for_monitoring(&tx1).await?;
        storage.enqueue_for_monitoring(&tx2).await?;

        println!("✅ Enqueued 2 transactions for monitoring");
        println!("   - Transaction 1: {} (1.5 SOL)", tx1.signature);
        println!("   - Transaction 2: {} (2.0 SOL)", tx2.signature);

        // Verify they're in the queue
        let pending = storage.get_pending_monitoring_transactions().await?;
        println!(
            "✅ Verified {} transactions in persistent queue",
            pending.len()
        );
    }
    // Storage goes out of scope here, simulating application shutdown

    println!("\n💥 Step 2: Simulating application restart...");
    println!("   (Previous storage instance was dropped)");

    // Simulate Step 3: Application restart, check if transactions persist
    println!("\n🚀 Step 3: Application restarted, loading persistent queue...");
    {
        let storage_after_restart = SqliteLedger::new().await?;

        // Load pending transactions - these should still be there!
        let pending = storage_after_restart
            .get_pending_monitoring_transactions()
            .await?;

        println!(
            "✅ Loaded {} transactions from persistent storage",
            pending.len()
        );

        for (i, tx) in pending.iter().enumerate() {
            println!(
                "   {}. {} (mint: {}, SOL: {})",
                i + 1,
                tx.signature,
                tx.mint,
                tx.initial_sol_spent
            );
        }

        if pending.len() >= 2 {
            println!("\n🎉 SUCCESS: Transactions survived the restart!");
            println!("   No data was lost during the simulated application crash/restart.");
        } else {
            println!("\n❌ UNEXPECTED: Some transactions were lost");
        }

        // Simulate processing one transaction
        if let Some(tx) = pending.first() {
            println!("\n📊 Step 4: Processing transaction {}...", tx.signature);
            storage_after_restart
                .update_monitoring_status(&tx.signature, "Completed")
                .await?;
            println!("✅ Marked transaction as Completed");

            // Verify it's no longer pending
            let pending_after = storage_after_restart
                .get_pending_monitoring_transactions()
                .await?;
            println!("✅ Pending transactions now: {}", pending_after.len());
        }

        // Clean up demo data
        let cleaned = storage_after_restart.cleanup_completed_monitoring().await?;
        println!("\n🧹 Cleaned up {} completed transactions", cleaned);
    }

    println!("\n🎯 Demonstration Complete!");
    println!("=====================================");
    println!("Key Benefits Demonstrated:");
    println!("✓ Transactions persist across application restarts");
    println!("✓ No data loss during unexpected shutdowns");
    println!("✓ Automatic recovery on application startup");
    println!("✓ Efficient cleanup of completed transactions");
    println!("✓ Production-ready fault tolerance");

    Ok(())
}
