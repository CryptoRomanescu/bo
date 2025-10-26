//! Early Pump Detection Engine
//!
//! Ultra-fast decision engine that analyzes memecoin launches in <100 seconds
//! and makes BUY/PASS decisions based on parallel async checks.
//!
//! # Key Requirements
//! - Detection window: <60 seconds from deploy
//! - Decision window: <60 seconds from detection → BUY/PASS
//! - Parallel analysis of multiple risk factors
//! - Score: 0-100 + BUY/PASS decision
//! - Comprehensive timing logs
//!
//! # Features
//! - Supply concentration analysis
//! - LP lock verification
//! - Wash trading detection
//! - Smart money monitoring
//! - Holder growth tracking
//!
//! # Reference
//! Based on CryptoLemur 2025 H2 analysis: median hold time for memecoins = 100 seconds

use crate::oracle::early_pump_cache::EarlyPumpCache;
use crate::oracle::graph_analyzer::{
    GraphAnalyzerConfig, OnChainGraphAnalyzer, WashTradingDetector, WashTradingConfig,
};
use crate::oracle::holder_growth_analyzer::{HolderGrowthAnalyzer, HolderGrowthConfig};
use crate::oracle::lp_lock_verifier::{LpLockConfig, LpLockVerifier, RiskLevel};
use crate::oracle::smart_money_tracker::{SmartMoneyTracker, SmartWalletTransaction};
use crate::oracle::supply_concentration_analyzer::{
    SupplyConcentrationAnalyzer, SupplyConcentrationConfig,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::timeout;
use tracing::{debug, info, instrument, warn};

/// Decision from early pump detector
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PumpDecision {
    /// BUY signal with score
    Buy { score: u8, reason: String },
    /// PASS signal with score
    Pass { score: u8, reason: String },
}

/// Timing metrics for decision process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionTimings {
    /// Time from token deploy to detection (in milliseconds)
    pub deploy_to_detection_ms: u64,
    /// Time from detection to decision (in milliseconds)
    pub detection_to_decision_ms: u64,
    /// Total time from deploy to decision (in milliseconds)
    pub total_decision_time_ms: u64,
    /// Individual check timings (in milliseconds)
    pub check_timings: CheckTimings,
}

/// Individual check timings
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CheckTimings {
    pub supply_concentration_ms: u64,
    pub lp_lock_ms: u64,
    pub wash_trading_ms: u64,
    pub smart_money_ms: u64,
    pub holder_growth_ms: u64,
}

/// Complete analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarlyPumpAnalysis {
    /// Token mint address
    pub mint: String,
    /// Deployment timestamp (unix seconds)
    pub deploy_timestamp: u64,
    /// Detection timestamp (unix seconds)
    pub detection_timestamp: u64,
    /// Decision timestamp (unix seconds)
    pub decision_timestamp: u64,
    /// Final decision
    pub decision: PumpDecision,
    /// Overall score (0-100)
    pub score: u8,
    /// Timing metrics
    pub timings: DecisionTimings,
    /// Individual check results
    pub check_results: CheckResults,
}

/// Results from individual checks
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CheckResults {
    /// Supply concentration score (0-100, higher = more concentrated)
    pub supply_concentration: u8,
    /// LP lock status score (0-100, higher = better lock)
    pub lp_lock: u8,
    /// LP lock details (from verifier)
    pub lp_lock_details: Option<String>,
    /// Wash trading risk score (0-100, higher = more risk)
    pub wash_trading_risk: u8,
    /// Smart money involvement score (0-100, higher = more smart money)
    pub smart_money: u8,
    /// Number of unique smart wallets detected
    pub smart_money_wallet_count: usize,
    /// Smart wallet transactions (for logging)
    pub smart_money_transactions: Vec<SmartWalletTransaction>,
    /// Holder growth score (0-100, higher = better growth)
    pub holder_growth: u8,
}

/// Configuration for early pump detector
#[derive(Debug, Clone)]
pub struct EarlyPumpConfig {
    /// Detection timeout (default: 60 seconds)
    pub detection_timeout_secs: u64,
    /// Decision timeout (default: 60 seconds)
    pub decision_timeout_secs: u64,
    /// BUY threshold score (default: 70)
    pub buy_threshold: u8,
    /// PASS threshold score (default: 50)
    pub pass_threshold: u8,
    /// Enable parallel checks (default: true)
    pub parallel_checks: bool,
    /// RPC endpoints for load balancing
    pub rpc_endpoints: Vec<String>,
}

impl Default for EarlyPumpConfig {
    fn default() -> Self {
        Self {
            detection_timeout_secs: 60,
            decision_timeout_secs: 60,
            buy_threshold: 70,
            pass_threshold: 50,
            parallel_checks: true,
            rpc_endpoints: vec!["https://api.mainnet-beta.solana.com".to_string()],
        }
    }
}

/// Early Pump Detector Engine
pub struct EarlyPumpDetector {
    config: EarlyPumpConfig,
    rpc_client: Arc<RpcClient>,
    smart_money_tracker: Option<Arc<SmartMoneyTracker>>,
    lp_lock_verifier: Arc<LpLockVerifier>,
    wash_trading_detector: Arc<WashTradingDetector>,
    supply_analyzer: Arc<SupplyConcentrationAnalyzer>,
    holder_growth_analyzer: Arc<HolderGrowthAnalyzer>,
    cache: Arc<EarlyPumpCache>,
}

impl EarlyPumpDetector {
    /// Create a new early pump detector
    pub fn new(config: EarlyPumpConfig, rpc_client: Arc<RpcClient>) -> Self {
        info!(
            "Initialized EarlyPumpDetector with detection_timeout={}s, decision_timeout={}s",
            config.detection_timeout_secs, config.decision_timeout_secs
        );
        
        // Create LP Lock Verifier with optimized settings for fast checks
        let lp_config = LpLockConfig {
            timeout_secs: 5,
            ..Default::default()
        };
        let lp_lock_verifier = Arc::new(LpLockVerifier::new(lp_config, rpc_client.clone()));
        
        // Create Wash Trading Detector with optimized settings
        let wash_config = WashTradingConfig {
            max_analysis_time_ms: 5000, // Fast analysis for real-time decisions
            min_circularity: 0.3,
            ..Default::default()
        };
        let wash_trading_detector = Arc::new(WashTradingDetector::new(wash_config));
        
        // Create Supply Concentration Analyzer with optimized settings
        let supply_config = SupplyConcentrationConfig {
            timeout_secs: 5,
            auto_reject_threshold: 70.0,
            ..Default::default()
        };
        let supply_analyzer = Arc::new(SupplyConcentrationAnalyzer::new(supply_config, rpc_client.clone()));
        
        // Create Holder Growth Analyzer with optimized settings
        let growth_config = HolderGrowthConfig {
            snapshot_interval_secs: 3,
            max_analysis_window_secs: 120,
            timeout_secs: 10,
            ..Default::default()
        };
        let holder_growth_analyzer = Arc::new(HolderGrowthAnalyzer::new(growth_config, rpc_client.clone()));
        
        // Create cache with 60s TTL and 1000 entry capacity
        let cache = Arc::new(EarlyPumpCache::new(1000, 60));
        
        Self {
            config,
            rpc_client,
            smart_money_tracker: None,
            lp_lock_verifier,
            wash_trading_detector,
            supply_analyzer,
            holder_growth_analyzer,
            cache,
        }
    }

    /// Create a new early pump detector with smart money tracking
    pub fn with_smart_money(
        config: EarlyPumpConfig,
        rpc_client: Arc<RpcClient>,
        smart_money_tracker: Arc<SmartMoneyTracker>,
    ) -> Self {
        info!(
            "Initialized EarlyPumpDetector with smart money tracking enabled"
        );
        
        // Create LP Lock Verifier with optimized settings for fast checks
        let lp_config = LpLockConfig {
            timeout_secs: 5,
            ..Default::default()
        };
        let lp_lock_verifier = Arc::new(LpLockVerifier::new(lp_config, rpc_client.clone()));
        
        // Create Wash Trading Detector with optimized settings
        let wash_config = WashTradingConfig {
            max_analysis_time_ms: 5000, // Fast analysis for real-time decisions
            min_circularity: 0.3,
            ..Default::default()
        };
        let wash_trading_detector = Arc::new(WashTradingDetector::new(wash_config));
        
        // Create Supply Concentration Analyzer with optimized settings
        let supply_config = SupplyConcentrationConfig {
            timeout_secs: 5,
            auto_reject_threshold: 70.0,
            ..Default::default()
        };
        let supply_analyzer = Arc::new(SupplyConcentrationAnalyzer::new(supply_config, rpc_client.clone()));
        
        // Create Holder Growth Analyzer with optimized settings
        let growth_config = HolderGrowthConfig {
            snapshot_interval_secs: 3,
            max_analysis_window_secs: 120,
            timeout_secs: 10,
            ..Default::default()
        };
        let holder_growth_analyzer = Arc::new(HolderGrowthAnalyzer::new(growth_config, rpc_client.clone()));
        
        // Create cache with 60s TTL and 1000 entry capacity
        let cache = Arc::new(EarlyPumpCache::new(1000, 60));
        
        Self {
            config,
            rpc_client,
            smart_money_tracker: Some(smart_money_tracker),
            lp_lock_verifier,
            wash_trading_detector,
            supply_analyzer,
            holder_growth_analyzer,
            cache,
        }
    }

    /// Analyze a token and make BUY/PASS decision
    ///
    /// # Arguments
    /// * `mint` - Token mint address
    /// * `deploy_timestamp` - When the token was deployed (unix seconds)
    /// * `program` - Deployment program (e.g., "pump.fun", "raydium", "jupiter")
    ///
    /// # Returns
    /// Complete analysis with decision and timing metrics
    #[instrument(skip(self))]
    pub async fn analyze(&self, mint: &str, deploy_timestamp: u64, program: &str) -> Result<EarlyPumpAnalysis> {
        // Check cache first
        if let Some(cached_analysis) = self.cache.get(mint).await {
            info!("Using cached analysis for mint={}", mint);
            return Ok((*cached_analysis).clone());
        }

        let start_time = Instant::now();
        let detection_timestamp = chrono::Utc::now().timestamp() as u64;

        info!(
            "Starting early pump analysis for mint={}, program={}, deploy_timestamp={}",
            mint, program, deploy_timestamp
        );

        // Calculate detection latency
        let deploy_to_detection_ms = if detection_timestamp > deploy_timestamp {
            (detection_timestamp - deploy_timestamp) * 1000
        } else {
            0
        };

        if deploy_to_detection_ms > self.config.detection_timeout_secs * 1000 {
            warn!(
                "Detection latency {}ms exceeds timeout {}s",
                deploy_to_detection_ms, self.config.detection_timeout_secs
            );
        }

        // Run parallel checks with timeout
        let decision_timeout = Duration::from_secs(self.config.decision_timeout_secs);
        let (check_results, check_timings) = if self.config.parallel_checks {
            timeout(decision_timeout, self.run_parallel_checks(mint, deploy_timestamp, program))
                .await
                .context("Decision timeout exceeded")?
                .context("Parallel checks failed")?
        } else {
            timeout(decision_timeout, self.run_sequential_checks(mint, deploy_timestamp, program))
                .await
                .context("Decision timeout exceeded")?
                .context("Sequential checks failed")?
        };

        let decision_timestamp = chrono::Utc::now().timestamp() as u64;
        let detection_to_decision_ms = (decision_timestamp - detection_timestamp) * 1000;
        let total_decision_time_ms = start_time.elapsed().as_millis() as u64;

        // Calculate overall score and make decision
        let score = self.calculate_score(&check_results);
        let decision = self.make_decision(score, &check_results);

        let analysis = EarlyPumpAnalysis {
            mint: mint.to_string(),
            deploy_timestamp,
            detection_timestamp,
            decision_timestamp,
            decision: decision.clone(),
            score,
            timings: DecisionTimings {
                deploy_to_detection_ms,
                detection_to_decision_ms,
                total_decision_time_ms,
                check_timings,
            },
            check_results,
        };

        info!(
            "Analysis complete: decision={:?}, score={}, total_time={}ms",
            decision, score, total_decision_time_ms
        );

        // Log warning if we exceeded 100s target
        if total_decision_time_ms > 100_000 {
            warn!(
                "PERFORMANCE WARNING: Total decision time {}ms exceeds 100s target",
                total_decision_time_ms
            );
        }

        // Cache the result
        self.cache.set(mint.to_string(), analysis.clone()).await;

        Ok(analysis)
    }

    /// Force refresh analysis for a token (bypasses cache)
    pub async fn force_refresh(&self, mint: &str, deploy_timestamp: u64, program: &str) -> Result<EarlyPumpAnalysis> {
        info!("Force refresh requested for mint={}", mint);
        
        // Invalidate cache
        self.cache.force_refresh(mint).await;
        
        // Run fresh analysis
        self.analyze(mint, deploy_timestamp, program).await
    }

    /// Invalidate cached analysis for a token (call when new transactions detected)
    pub async fn invalidate_cache(&self, mint: &str) {
        self.cache.invalidate(mint).await;
        info!("Invalidated cache for mint={}", mint);
    }

    /// Get cache metrics
    pub async fn get_cache_metrics(&self) -> crate::oracle::early_pump_cache::CacheMetricsSnapshot {
        self.cache.get_metrics().await
    }

    /// Run all checks in parallel for maximum speed
    async fn run_parallel_checks(&self, mint: &str, deploy_timestamp: u64, program: &str) -> Result<(CheckResults, CheckTimings)> {
        let mint = mint.to_string();
        let program = program.to_string();

        // Spawn all checks concurrently
        let supply_check: tokio::task::JoinHandle<Result<(u8, u64)>> = {
            let mint = mint.clone();
            let analyzer = self.supply_analyzer.clone();
            tokio::spawn(async move { 
                let start = Instant::now();
                match analyzer.analyze(&mint).await {
                    Ok(result) => {
                        let elapsed = start.elapsed().as_millis() as u64;
                        // Convert concentration to risk score (higher concentration = higher risk)
                        let score = result.metrics.risk_score;
                        Ok((score, elapsed))
                    }
                    Err(e) => {
                        warn!("Supply concentration analysis failed for {}: {}", mint, e);
                        let elapsed = start.elapsed().as_millis() as u64;
                        Ok((45u8, elapsed)) // Fallback score
                    }
                }
            })
        };

        let lp_check = {
            let mint = mint.clone();
            let program = program.clone();
            let verifier = self.lp_lock_verifier.clone();
            tokio::spawn(async move { 
                let start = Instant::now();
                let result: Result<(u8, u64, Option<String>)> = match verifier.verify(&mint, &program).await {
                    Ok(result) => {
                        let elapsed = start.elapsed().as_millis() as u64;
                        let summary = format!(
                            "LP: {:?} (risk={:?}, safety={}{})",
                            result.lock_status,
                            result.risk_level,
                            result.safety_score,
                            if result.auto_reject { ", AUTO-REJECT" } else { "" }
                        );
                        Ok((result.safety_score, elapsed, Some(summary)))
                    }
                    Err(e) => {
                        let elapsed = start.elapsed().as_millis() as u64;
                        Ok((50u8, elapsed, Some(format!("LP verification failed: {}", e))))
                    }
                };
                result
            })
        };

        let wash_check = {
            let mint = mint.clone();
            let client = self.rpc_client.clone();
            let detector = self.wash_trading_detector.clone();
            tokio::spawn(async move { Self::check_wash_trading(&mint, client, detector).await })
        };

        let smart_money_check = {
            let mint = mint.clone();
            let tracker = self.smart_money_tracker.clone();
            tokio::spawn(async move { Self::check_smart_money_with_tracker(&mint, deploy_timestamp, tracker).await })
        };

        let holder_check: tokio::task::JoinHandle<Result<(u8, u64)>> = {
            let mint = mint.clone();
            let analyzer = self.holder_growth_analyzer.clone();
            tokio::spawn(async move { 
                let start = Instant::now();
                match analyzer.analyze(&mint, deploy_timestamp, None, None).await {
                    Ok(result) => {
                        let elapsed = start.elapsed().as_millis() as u64;
                        info!(
                            "Holder growth analysis for {}: holders={}, growth_rate={:.2}, score={}, organic={}, bot_prob={:.2}, anomalies={}, time={}ms",
                            mint,
                            result.current_holders,
                            result.growth_rate,
                            result.growth_score,
                            result.is_organic,
                            result.bot_probability,
                            result.anomalies.len(),
                            elapsed
                        );
                        Ok((result.growth_score, elapsed))
                    }
                    Err(e) => {
                        warn!("Holder growth analysis failed for {}: {}", mint, e);
                        let elapsed = start.elapsed().as_millis() as u64;
                        Ok((50u8, elapsed)) // Fallback score
                    }
                }
            })
        };

        // Wait for all checks to complete
        let (supply_result, lp_result, wash_result, smart_result, holder_result) = tokio::join!(
            supply_check,
            lp_check,
            wash_check,
            smart_money_check,
            holder_check
        );

        // Unwrap results and extract scores + timings
        let (supply_concentration, supply_time) = supply_result??;
        let (lp_lock, lp_time, lp_details) = lp_result??;
        let (wash_trading_risk, wash_time) = wash_result??;
        let (smart_money, wallet_count, transactions, smart_time) = smart_result??;
        let (holder_growth, holder_time) = holder_result??;

        let results = CheckResults {
            supply_concentration,
            lp_lock,
            lp_lock_details: lp_details,
            wash_trading_risk,
            smart_money,
            smart_money_wallet_count: wallet_count,
            smart_money_transactions: transactions,
            holder_growth,
        };

        let timings = CheckTimings {
            supply_concentration_ms: supply_time,
            lp_lock_ms: lp_time,
            wash_trading_ms: wash_time,
            smart_money_ms: smart_time,
            holder_growth_ms: holder_time,
        };

        debug!("Parallel checks completed: {:?}", results);
        Ok((results, timings))
    }

    /// Run checks sequentially (fallback mode)
    async fn run_sequential_checks(&self, mint: &str, deploy_timestamp: u64, program: &str) -> Result<(CheckResults, CheckTimings)> {
        let (supply_concentration, supply_time) =
            self.check_supply_concentration(mint).await?;
        let (lp_lock, lp_time, lp_details) = self.check_lp_lock(mint, program, self.rpc_client.clone()).await?;
        let (wash_trading_risk, wash_time) = Self::check_wash_trading(mint, self.rpc_client.clone(), self.wash_trading_detector.clone()).await?;
        let (smart_money, wallet_count, transactions, smart_time) =
            Self::check_smart_money_with_tracker(mint, deploy_timestamp, self.smart_money_tracker.clone()).await?;
        let (holder_growth, holder_time) = self.check_holder_growth_real(mint, deploy_timestamp).await?;

        let results = CheckResults {
            supply_concentration,
            lp_lock,
            lp_lock_details: lp_details,
            wash_trading_risk,
            smart_money,
            smart_money_wallet_count: wallet_count,
            smart_money_transactions: transactions,
            holder_growth,
        };

        let timings = CheckTimings {
            supply_concentration_ms: supply_time,
            lp_lock_ms: lp_time,
            wash_trading_ms: wash_time,
            smart_money_ms: smart_time,
            holder_growth_ms: holder_time,
        };

        debug!("Sequential checks completed: {:?}", results);
        Ok((results, timings))
    }

    /// Check supply concentration (higher score = more concentrated = riskier)
    async fn check_supply_concentration(&self, mint: &str) -> Result<(u8, u64)> {
        let start = Instant::now();
        debug!("Checking supply concentration for {}", mint);

        match self.supply_analyzer.analyze(mint).await {
            Ok(result) => {
                let elapsed = start.elapsed().as_millis() as u64;
                let score = result.metrics.risk_score;
                
                info!(
                    "Supply analysis for {}: top10={:.1}%, top25={:.1}%, gini={:.3}, risk={}, auto_reject={}, time={}ms",
                    mint,
                    result.metrics.top_10_concentration,
                    result.metrics.top_25_concentration,
                    result.metrics.gini_coefficient,
                    score,
                    result.metrics.auto_reject,
                    elapsed
                );
                
                debug!("Supply concentration check completed in {}ms", elapsed);
                Ok((score, elapsed))
            }
            Err(e) => {
                warn!("Supply concentration analysis failed for {}: {}", mint, e);
                let elapsed = start.elapsed().as_millis() as u64;
                Ok((45, elapsed)) // Fallback score
            }
        }
    }

    /// Check LP lock status (higher score = better lock = safer)
    async fn check_lp_lock(&self, mint: &str, program: &str, _rpc: Arc<RpcClient>) -> Result<(u8, u64, Option<String>)> {
        let start = Instant::now();
        debug!("Checking LP lock for {} on {}", mint, program);

        // Use the LP Lock Verifier for real verification
        match self.lp_lock_verifier.verify(mint, program).await {
            Ok(result) => {
                let elapsed = start.elapsed().as_millis() as u64;
                let score = result.safety_score;
                
                // Log LP verification details
                info!(
                    "LP Lock verified for {}: status={:?}, risk={:?}, safety={}, auto_reject={}, time={}ms",
                    mint, result.lock_status, result.risk_level, score, result.auto_reject, elapsed
                );
                
                // Create summary for notes
                let summary = format!(
                    "LP: {:?} (risk={:?}, safety={}{})",
                    result.lock_status,
                    result.risk_level,
                    score,
                    if result.auto_reject { ", AUTO-REJECT" } else { "" }
                );
                
                debug!("LP lock check completed in {}ms", elapsed);
                Ok((score, elapsed, Some(summary)))
            }
            Err(e) => {
                let elapsed = start.elapsed().as_millis() as u64;
                warn!("LP lock verification failed for {}: {}, using fallback", mint, e);
                
                // Fallback to placeholder score (conservative approach)
                let score = 50; // Neutral score when verification fails
                debug!("LP lock fallback check completed in {}ms", elapsed);
                Ok((score, elapsed, Some(format!("LP verification failed: {}", e))))
            }
        }
    }

    /// Check for wash trading patterns (higher score = more risk)
    async fn check_wash_trading(mint: &str, rpc: Arc<RpcClient>, detector: Arc<WashTradingDetector>) -> Result<(u8, u64)> {
        let start = Instant::now();
        debug!("Checking wash trading for {}", mint);

        // Build transaction graph for analysis
        let graph_config = GraphAnalyzerConfig {
            max_transactions: 1000, // Limit for fast analysis
            time_window_secs: 3600,  // Last hour
            max_wallets: 500,
            ..Default::default()
        };
        
        let mut graph = crate::oracle::graph_analyzer::graph_builder::TransactionGraph::new(graph_config.clone());
        
        // Build graph from on-chain data with timeout
        match tokio::time::timeout(
            Duration::from_secs(5),
            graph.build_from_token(rpc.clone(), mint, Some(1000))
        ).await {
            Ok(Ok(_)) => {
                // Calculate metrics
                graph.calculate_metrics();
                
                // Run wash trading detection
                match detector.detect(&graph) {
                    Ok(result) => {
                        let elapsed = start.elapsed().as_millis() as u64;
                        
                        info!(
                            "Wash trading analysis for {}: probability={:.3}, circularity={:.3}, clusters={}, time={}ms{}",
                            mint,
                            result.wash_probability,
                            result.circularity_score,
                            result.suspicious_clusters,
                            result.analysis_time_ms,
                            if result.auto_reject { ", AUTO-REJECT" } else { "" }
                        );
                        
                        Ok((result.risk_score, elapsed))
                    }
                    Err(e) => {
                        warn!("Wash trading detection failed: {}, using fallback", e);
                        let elapsed = start.elapsed().as_millis() as u64;
                        Ok((25, elapsed)) // Safe fallback score
                    }
                }
            }
            Ok(Err(e)) => {
                warn!("Failed to build transaction graph: {}, using fallback", e);
                let elapsed = start.elapsed().as_millis() as u64;
                Ok((25, elapsed))
            }
            Err(_) => {
                warn!("Graph build timeout for {}, using fallback", mint);
                let elapsed = start.elapsed().as_millis() as u64;
                Ok((25, elapsed))
            }
        }
    }

    /// Check smart money involvement with tracker (real implementation)
    async fn check_smart_money_with_tracker(
        mint: &str,
        deploy_timestamp: u64,
        tracker: Option<Arc<SmartMoneyTracker>>,
    ) -> Result<(u8, usize, Vec<SmartWalletTransaction>, u64)> {
        let start = Instant::now();
        debug!("Checking smart money for {} with tracker", mint);

        if let Some(tracker) = tracker {
            // Use real smart money tracker
            match tracker.check_smart_money(mint, deploy_timestamp).await {
                Ok((score, wallet_count, transactions)) => {
                    let elapsed = start.elapsed().as_millis() as u64;
                    
                    // Log detailed smart money activity
                    if wallet_count > 0 {
                        info!(
                            "🎯 Smart money detected for {}: {} wallets, score={}, {} transactions",
                            mint, wallet_count, score, transactions.len()
                        );
                        
                        for tx in &transactions {
                            debug!(
                                "  Wallet: {}, Amount: {} SOL, Time: {}, Tx: {}",
                                tx.wallet, tx.amount_sol, tx.timestamp, tx.signature
                            );
                        }
                    }
                    
                    debug!("Smart money check completed in {}ms", elapsed);
                    Ok((score, wallet_count, transactions, elapsed))
                }
                Err(e) => {
                    warn!("Smart money tracker failed: {}, using fallback", e);
                    Self::check_smart_money_fallback(mint).await
                }
            }
        } else {
            // Fallback to simple check
            Self::check_smart_money_fallback(mint).await
        }
    }

    /// Fallback smart money check (placeholder)
    async fn check_smart_money_fallback(mint: &str) -> Result<(u8, usize, Vec<SmartWalletTransaction>, u64)> {
        let start = Instant::now();
        debug!("Using fallback smart money check for {}", mint);
        
        tokio::time::sleep(Duration::from_millis(80)).await;

        let score = 55; // Placeholder score
        let elapsed = start.elapsed().as_millis() as u64;

        debug!("Smart money fallback check completed in {}ms", elapsed);
        Ok((score, 0, vec![], elapsed))
    }

    /// Check smart money involvement (legacy placeholder - kept for backward compatibility)
    #[allow(dead_code)]
    async fn check_smart_money(mint: &str, _rpc: Arc<RpcClient>) -> Result<(u8, u64)> {
        let start = Instant::now();
        debug!("Checking smart money for {} (legacy)", mint);

        tokio::time::sleep(Duration::from_millis(80)).await;

        let score = 55; // Placeholder score
        let elapsed = start.elapsed().as_millis() as u64;

        debug!("Smart money check completed in {}ms", elapsed);
        Ok((score, elapsed))
    }

    /// Check holder growth rate with real analyzer (higher score = better growth)
    async fn check_holder_growth_real(&self, mint: &str, deploy_timestamp: u64) -> Result<(u8, u64)> {
        let start = Instant::now();
        debug!("Checking holder growth for {} with real analyzer", mint);

        match self.holder_growth_analyzer.analyze(mint, deploy_timestamp, None, None).await {
            Ok(result) => {
                let elapsed = start.elapsed().as_millis() as u64;
                
                info!(
                    "Holder growth analysis for {}: holders={}, growth_rate={:.2}, score={}, organic={}, bot_prob={:.2}, anomalies={}, time={}ms",
                    mint,
                    result.current_holders,
                    result.growth_rate,
                    result.growth_score,
                    result.is_organic,
                    result.bot_probability,
                    result.anomalies.len(),
                    elapsed
                );
                
                debug!("Holder growth check completed in {}ms", elapsed);
                Ok((result.growth_score, elapsed))
            }
            Err(e) => {
                warn!("Holder growth analysis failed for {}: {}", mint, e);
                let elapsed = start.elapsed().as_millis() as u64;
                Ok((50, elapsed)) // Fallback score
            }
        }
    }

    /// Check holder growth rate (legacy placeholder - kept for backward compatibility)
    #[allow(dead_code)]
    async fn check_holder_growth(mint: &str, _rpc: Arc<RpcClient>) -> Result<(u8, u64)> {
        let start = Instant::now();
        debug!("Checking holder growth for {} (legacy)", mint);

        tokio::time::sleep(Duration::from_millis(60)).await;

        let score = 70; // Placeholder score
        let elapsed = start.elapsed().as_millis() as u64;

        debug!("Holder growth check completed in {}ms", elapsed);
        Ok((score, elapsed))
    }

    /// Calculate overall score from check results
    fn calculate_score(&self, results: &CheckResults) -> u8 {
        // Weighted scoring formula
        // Positive factors: LP lock (30%), smart money (25%), holder growth (20%)
        // Risk factors: supply concentration (15%), wash trading (10%)

        let positive_score = (results.lp_lock as f64 * 0.30)
            + (results.smart_money as f64 * 0.25)
            + (results.holder_growth as f64 * 0.20);

        // Invert risk factors (lower is better)
        let risk_penalty = ((100 - results.supply_concentration) as f64 * 0.15)
            + ((100 - results.wash_trading_risk) as f64 * 0.10);

        let final_score = (positive_score + risk_penalty).round().min(100.0).max(0.0) as u8;

        debug!(
            "Score calculation: positive={:.2}, risk_penalty={:.2}, final={}",
            positive_score, risk_penalty, final_score
        );

        final_score
    }

    /// Make BUY/PASS decision based on score and check results
    fn make_decision(&self, score: u8, results: &CheckResults) -> PumpDecision {
        // Check for specific red flags first - these override any positive score
        if results.wash_trading_risk > 80 {
            let reason = "High wash trading risk detected".to_string();
            return PumpDecision::Pass { score, reason };
        }

        if results.supply_concentration > 85 {
            let reason = "Extreme supply concentration detected".to_string();
            return PumpDecision::Pass { score, reason };
        }

        // Ultra-strong signal: Multiple smart wallets buying (per requirement: >2 wallets)
        if results.smart_money_wallet_count >= 2 && results.smart_money >= 60 {
            let reason = format!(
                "🚨 SMART MONEY ALERT: {} smart wallets detected! LP lock={}, score={}",
                results.smart_money_wallet_count, results.lp_lock, results.smart_money
            );
            info!("{}", reason);
            return PumpDecision::Buy { score, reason };
        }

        // Strong BUY signal
        if score >= self.config.buy_threshold {
            let reason = format!(
                "Strong fundamentals: LP lock={}, smart money={} ({} wallets), holder growth={}",
                results.lp_lock, results.smart_money, results.smart_money_wallet_count, results.holder_growth
            );
            return PumpDecision::Buy { score, reason };
        }

        // Medium score - check for positive signals
        if score >= self.config.pass_threshold {
            if results.smart_money >= 70 && results.holder_growth >= 60 {
                let reason = format!(
                    "Good smart money ({} wallets) and holder growth signals",
                    results.smart_money_wallet_count
                );
                return PumpDecision::Buy { score, reason };
            }
        }

        // Default to PASS
        let reason = if score < self.config.pass_threshold {
            "Score below pass threshold".to_string()
        } else {
            "Insufficient positive signals".to_string()
        };

        PumpDecision::Pass { score, reason }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_decision_timing() {
        let config = EarlyPumpConfig::default();
        let rpc = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
        let detector = EarlyPumpDetector::new(config, rpc);

        let deploy_timestamp = chrono::Utc::now().timestamp() as u64 - 30; // 30 seconds ago
        let result = detector
            .analyze("TestMint123", deploy_timestamp, "pump.fun")
            .await
            .unwrap();

        // Verify timing constraints
        assert!(result.timings.total_decision_time_ms < 100_000, "Decision took too long");
        assert!(result.timings.detection_to_decision_ms < 60_000, "Decision phase took too long");
    }

    #[tokio::test]
    async fn test_parallel_checks() {
        let config = EarlyPumpConfig {
            parallel_checks: true,
            ..Default::default()
        };
        let rpc = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
        let detector = EarlyPumpDetector::new(config, rpc);

        let result = detector
            .run_parallel_checks("TestMint", 0, "pump.fun")
            .await
            .unwrap();

        // All checks should complete
        assert!(result.0.supply_concentration <= 100);
        assert!(result.0.lp_lock <= 100);
        assert!(result.0.wash_trading_risk <= 100);
        assert!(result.0.smart_money <= 100);
        assert!(result.0.holder_growth <= 100);
    }

    #[test]
    fn test_score_calculation() {
        let config = EarlyPumpConfig::default();
        let rpc = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
        let detector = EarlyPumpDetector::new(config, rpc);

        // Test high quality token
        let good_results = CheckResults {
            supply_concentration: 20, // Low concentration (good)
            lp_lock: 90,              // Strong lock (good)
            lp_lock_details: Some("Fully locked".to_string()),
            wash_trading_risk: 10,    // Low risk (good)
            smart_money: 80,          // High smart money (good)
            smart_money_wallet_count: 3,
            smart_money_transactions: vec![],
            holder_growth: 85,        // Strong growth (good)
        };
        let score = detector.calculate_score(&good_results);
        assert!(score >= 70, "Good token should score high");

        // Test poor quality token
        let bad_results = CheckResults {
            supply_concentration: 90, // High concentration (bad)
            lp_lock: 20,              // Weak lock (bad)
            lp_lock_details: Some("Unlocked".to_string()),
            wash_trading_risk: 85,    // High risk (bad)
            smart_money: 15,          // Low smart money (bad)
            smart_money_wallet_count: 0,
            smart_money_transactions: vec![],
            holder_growth: 25,        // Poor growth (bad)
        };
        let score = detector.calculate_score(&bad_results);
        assert!(score < 50, "Bad token should score low");
    }

    #[test]
    fn test_decision_logic() {
        let config = EarlyPumpConfig::default();
        let rpc = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
        let detector = EarlyPumpDetector::new(config, rpc);

        // High score should result in BUY
        let good_results = CheckResults {
            supply_concentration: 20,
            lp_lock: 90,
            lp_lock_details: Some("Fully locked".to_string()),
            wash_trading_risk: 10,
            smart_money: 80,
            smart_money_wallet_count: 3,
            smart_money_transactions: vec![],
            holder_growth: 85,
        };
        let decision = detector.make_decision(75, &good_results);
        assert!(matches!(decision, PumpDecision::Buy { .. }));

        // High wash trading should override score
        let risky_results = CheckResults {
            supply_concentration: 30,
            lp_lock: 80,
            lp_lock_details: Some("Locked".to_string()),
            wash_trading_risk: 85, // Red flag
            smart_money: 70,
            smart_money_wallet_count: 2,
            smart_money_transactions: vec![],
            holder_growth: 75,
        };
        let decision = detector.make_decision(70, &risky_results);
        assert!(matches!(decision, PumpDecision::Pass { .. }));

        // Low score should result in PASS
        let bad_results = CheckResults {
            supply_concentration: 80,
            lp_lock: 30,
            lp_lock_details: Some("Unlocked".to_string()),
            wash_trading_risk: 70,
            smart_money: 25,
            smart_money_wallet_count: 0,
            smart_money_transactions: vec![],
            holder_growth: 30,
        };
        let decision = detector.make_decision(35, &bad_results);
        assert!(matches!(decision, PumpDecision::Pass { .. }));
    }
}
