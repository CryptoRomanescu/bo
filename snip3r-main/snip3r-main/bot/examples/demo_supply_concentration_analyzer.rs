//! Demo: Supply Concentration Analyzer
//!
//! This demo showcases the supply concentration analyzer module that analyzes
//! token holder distribution to detect extreme concentration patterns.
//!
//! ## Features Demonstrated
//! - Top 10 and top 25 holder concentration analysis
//! - Gini coefficient calculation for supply distribution
//! - Auto-reject logic for high concentration (>70% top 10)
//! - Whale detection (holders with >5% supply)
//! - Risk scoring system
//! - Performance metrics (<5s analysis)
//!
//! ## Usage
//! ```bash
//! cargo run --example demo_supply_concentration_analyzer
//! ```

use h_5n1p3r::oracle::{
    ConcentrationMetrics, HolderInfo, SupplyAnalysisResult, SupplyConcentrationAnalyzer,
    SupplyConcentrationConfig,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt().with_env_filter("info").init();

    println!("═══════════════════════════════════════════════════════════");
    println!("   Supply Concentration Analyzer Demo");
    println!("═══════════════════════════════════════════════════════════\n");

    // Create RPC client
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let rpc_client = Arc::new(RpcClient::new(rpc_url));

    println!("📊 Initializing Supply Concentration Analyzer...\n");

    // Create analyzer with default configuration
    let config = SupplyConcentrationConfig {
        timeout_secs: 5,
        auto_reject_threshold: 70.0,
        whale_threshold: 5.0,
        enable_cache: true,
        ..Default::default()
    };

    let analyzer = SupplyConcentrationAnalyzer::new(config, rpc_client);

    println!("✅ Analyzer initialized with configuration:");
    println!("   • Auto-reject threshold: 70%");
    println!("   • Whale threshold: 5%");
    println!("   • Timeout: 5 seconds");
    println!("   • Caching: enabled\n");

    // Demo scenarios with simulated data
    demo_scenario_1_safe_token();
    demo_scenario_2_risky_token();
    demo_scenario_3_extreme_concentration();
    demo_scenario_4_gini_coefficient();

    // Note about real-world usage
    println!("\n═══════════════════════════════════════════════════════════");
    println!("   Real-World Usage");
    println!("═══════════════════════════════════════════════════════════\n");
    println!("To analyze a real token on Solana:");
    println!("  let result = analyzer.analyze(\"<token_mint_address>\").await?;");
    println!("\nThe analyzer will:");
    println!("  1. Fetch all token accounts from on-chain data");
    println!("  2. Calculate top 10/25 holder concentrations");
    println!("  3. Compute Gini coefficient");
    println!("  4. Detect whales (>5% holders)");
    println!("  5. Generate risk score (0-100)");
    println!("  6. Apply auto-reject if >70% concentration\n");

    println!("Performance target: <5 seconds per token analysis ⚡");
    println!("\n═══════════════════════════════════════════════════════════\n");

    Ok(())
}

/// Demo Scenario 1: Safe Token (Low Concentration)
fn demo_scenario_1_safe_token() {
    println!("═══════════════════════════════════════════════════════════");
    println!("   Scenario 1: Safe Token (Low Concentration)");
    println!("═══════════════════════════════════════════════════════════\n");

    let metrics = ConcentrationMetrics {
        total_holders: 5000,
        top_10_concentration: 25.5,
        top_25_concentration: 42.3,
        gini_coefficient: 0.35,
        whale_count: 2,
        auto_reject: false,
        risk_score: 28,
    };

    print_analysis_summary(&metrics, "SafeToken123");
    print_interpretation("SAFE", "Low concentration, well-distributed");
}

/// Demo Scenario 2: Risky Token (High Concentration)
fn demo_scenario_2_risky_token() {
    println!("\n═══════════════════════════════════════════════════════════");
    println!("   Scenario 2: Risky Token (High Concentration)");
    println!("═══════════════════════════════════════════════════════════\n");

    let metrics = ConcentrationMetrics {
        total_holders: 1200,
        top_10_concentration: 75.8,
        top_25_concentration: 88.2,
        gini_coefficient: 0.82,
        whale_count: 7,
        auto_reject: true,
        risk_score: 82,
    };

    print_analysis_summary(&metrics, "RiskyToken456");
    print_interpretation(
        "DANGEROUS",
        "High concentration, 99% dump risk - AUTO REJECT",
    );
}

/// Demo Scenario 3: Extreme Concentration (Rug Pull Risk)
fn demo_scenario_3_extreme_concentration() {
    println!("\n═══════════════════════════════════════════════════════════");
    println!("   Scenario 3: Extreme Concentration (Rug Pull Risk)");
    println!("═══════════════════════════════════════════════════════════\n");

    let metrics = ConcentrationMetrics {
        total_holders: 350,
        top_10_concentration: 92.5,
        top_25_concentration: 97.8,
        gini_coefficient: 0.95,
        whale_count: 9,
        auto_reject: true,
        risk_score: 96,
    };

    print_analysis_summary(&metrics, "RugToken789");
    print_interpretation(
        "EXTREME DANGER",
        "Top 10 wallets control >90% - Classic rug pull setup - AUTO REJECT",
    );
}

/// Demo Scenario 4: Gini Coefficient Examples
fn demo_scenario_4_gini_coefficient() {
    println!("\n═══════════════════════════════════════════════════════════");
    println!("   Scenario 4: Gini Coefficient Interpretation");
    println!("═══════════════════════════════════════════════════════════\n");

    println!("The Gini coefficient measures supply inequality:");
    println!("  • 0.0 = Perfect equality (everyone has same amount)");
    println!("  • 1.0 = Perfect inequality (one holder has everything)\n");

    let examples = vec![
        (0.20, "Very equal distribution", "🟢 Excellent"),
        (0.40, "Fair distribution", "🟢 Good"),
        (0.60, "Moderate inequality", "🟡 Acceptable"),
        (0.75, "High inequality", "🟠 Concerning"),
        (0.90, "Extreme inequality", "🔴 Dangerous"),
    ];

    println!("┌───────────┬─────────────────────────┬──────────────┐");
    println!("│ Gini Coef │ Description             │ Assessment   │");
    println!("├───────────┼─────────────────────────┼──────────────┤");
    for (gini, desc, assessment) in examples {
        println!("│ {:.2}      │ {:<23} │ {:<12} │", gini, desc, assessment);
    }
    println!("└───────────┴─────────────────────────┴──────────────┘");
}

/// Print analysis summary
fn print_analysis_summary(metrics: &ConcentrationMetrics, token_name: &str) {
    println!("Token: {}", token_name);
    println!("─────────────────────────────────────────────────────────────\n");

    println!("📊 Holder Statistics:");
    println!("  • Total Holders: {}", metrics.total_holders);
    println!("  • Whale Count (>5%): {}", metrics.whale_count);
    println!();

    println!("📈 Concentration Metrics:");
    println!("  • Top 10 Holders: {:.1}%", metrics.top_10_concentration);
    println!("  • Top 25 Holders: {:.1}%", metrics.top_25_concentration);
    println!("  • Gini Coefficient: {:.3}", metrics.gini_coefficient);
    println!();

    println!("⚠️  Risk Assessment:");
    println!("  • Risk Score: {}/100", metrics.risk_score);
    println!(
        "  • Auto-Reject: {}",
        if metrics.auto_reject {
            "YES ⛔"
        } else {
            "NO ✅"
        }
    );
    println!();
}

/// Print interpretation
fn print_interpretation(status: &str, reason: &str) {
    let icon = if status.contains("SAFE") {
        "✅"
    } else if status.contains("EXTREME") {
        "🚨"
    } else {
        "⚠️"
    };

    println!("💡 Interpretation:");
    println!("  {} Status: {}", icon, status);
    println!("  📝 Reason: {}", reason);
}
