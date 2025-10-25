//! Oracle module - DecisionLedger and Pillar II components
//!
//! This module contains the DecisionLedger operational memory system (Pillar I)
//! and the PerformanceMonitor/StrategyOptimizer feedback loop (Pillar II).

pub mod bot_bundle_detector; // Universe-Class Bot Activity & Bundle Detection
pub mod circuit_breaker; // Advanced circuit breaker for endpoint health
pub mod data_sources; // For MarketRegimeDetector
pub mod decision_ledger;
pub mod dex_screener_tracker; // DEX Screener Trending Integration
pub mod early_pump_detector; // Ultra-Fast Early Pump Detection Engine (<100s decisions)
pub mod ensemble; // Quantum Ensemble Oracle System
pub mod feature_engineering; // ML Feature Engineering Framework
pub mod graph_analyzer;
pub mod intelligence; // Multi-Modal Intelligence Layer (Sentiment Analysis)
pub mod lp_lock_verifier; // Universe-Class LP Lock and Burn Verifier
pub mod market_regime_detector; // Pillar III
pub mod metacognition; // Lightweight Metacognitive Awareness
pub mod pattern_memory; // Temporal Pattern Recognition
pub mod performance_monitor;
pub mod quantum_oracle; // Universe-Class Predictive Oracle
pub mod smart_money_tracker; // Real-time Smart Money Monitoring (Nansen/Birdeye Integration)
pub mod storage; // Storage abstraction layer
pub mod strategy_optimizer;
pub mod supply_concentration_analyzer; // Universe-Class Token Supply Concentration Analyzer
pub mod tiered_storage; // Advanced tiered storage system
pub mod token_detector; // Ultra-Fast Token Detection System (<1s latency)
pub mod transaction_monitor;
pub mod types;
pub mod types_old; // Old types that are still in use // On-Chain Graph Analyzer (Pump/Wash Detection)

// Re-export main types
pub use types::{
    DecisionRecordSender,
    FeatureWeights,
    // Pillar III types
    MarketRegime,
    OptimizedParameters,
    OptimizedParametersReceiver,
    OptimizedParametersSender,
    OracleConfig,
    Outcome,
    OutcomeUpdateSender,
    PerformanceReport,
    PerformanceReportReceiver,
    PerformanceReportSender,
    RegimeSpecificParameters,
    ScoreThresholds,
    ScoredCandidate,
    TransactionRecord,
};

// Re-export storage abstraction
pub use storage::{LedgerStorage, SqliteLedger, SqliteLedgerNormalized};

// Re-export tiered storage system
pub use tiered_storage::{
    AutoTieringCoordinator, ColdArchiveStorage, HotMemoryStorage, StorageTier, TierMetrics,
    TierMonitor, TieringConfig, WarmSqliteStorage,
};

// Re-export circuit breaker
pub use circuit_breaker::{
    CircuitBreaker, CircuitBreakerConfig, EndpointHealth, EndpointHealthStats, EndpointState,
};

// Re-export DEX Screener tracker
pub use dex_screener_tracker::{
    DexScreenerConfig, DexScreenerTracker, TrendingEvent, TrendingEventRecord, TrendingMetrics,
    TrendingPosition,
};

// Re-export key components
pub use data_sources::OracleDataSources; // For MarketRegimeDetector
pub use decision_ledger::DecisionLedger;
pub use early_pump_detector::{
    CheckResults, CheckTimings, DecisionTimings, EarlyPumpAnalysis, EarlyPumpConfig,
    EarlyPumpDetector, PumpDecision,
}; // Ultra-Fast Early Pump Detection Engine
pub use ensemble::{
    AggressiveOracle, ArbiterDecision, ConservativeOracle, EnsembleConfig, EnsembleCoordinator,
    EnsembleStats, MetaArbiter, OracleStrategy, QuantumVoter, StrategyConfig, StrategyPerformance,
    TradeOutcome, VotingResult,
}; // Quantum Ensemble Oracle System
pub use feature_engineering::{
    FeatureCatalog, FeatureCategory, FeatureEngineeringPipeline, FeatureExtractor,
    FeatureNormalizer, FeatureSelector, FeatureStore, FeatureVector,
}; // ML Feature Engineering Framework
pub use graph_analyzer::{
    AnalysisResult, AnalysisSummary, GraphAnalyzerConfig, GraphMetrics, GraphPatternMatch,
    OnChainGraphAnalyzer, PatternType, TransactionEdge, WalletAddress, WalletCluster,
};
pub use intelligence::{
    DiscordMonitor, SentimentCache, SentimentConfig, SentimentData, SentimentEngine,
    SentimentMetrics, SentimentScore, SocialSource, TelegramScraper, TokenSentiment, TwitterClient,
}; // Multi-Modal Intelligence Layer
pub use lp_lock_verifier::{
    LockStatus, LpLockConfig, LpLockVerifier, LpVerificationResult, RiskLevel,
}; // LP Lock and Burn Verifier
pub use market_regime_detector::MarketRegimeDetector; // Pillar III
pub use metacognition::{CalibrationStats, ConfidenceCalibrator, DecisionOutcome}; // Metacognitive Awareness
pub use pattern_memory::{Observation, Pattern, PatternMatch, PatternMemory, PatternMemoryStats}; // Temporal Pattern Recognition
pub use performance_monitor::PerformanceMonitor;
pub use quantum_oracle::PredictiveOracle; // Universe-Class Predictive Oracle
pub use smart_money_tracker::{
    DataSource, SmartMoneyAlert, SmartMoneyConfig, SmartMoneyTracker, SmartWalletProfile,
    SmartWalletTransaction, TransactionType,
}; // Real-time Smart Money Monitoring
pub use strategy_optimizer::StrategyOptimizer;
pub use supply_concentration_analyzer::{
    ConcentrationMetrics, HolderInfo, SupplyAnalysisResult, SupplyConcentrationAnalyzer,
    SupplyConcentrationConfig,
}; // Universe-Class Token Supply Concentration Analyzer
pub use token_detector::{
    DetectionEvent, DetectionMetrics, DetectionSensitivity, DetectionSource, TokenDetector,
    TokenDetectorConfig,
}; // Ultra-Fast Token Detection System
pub use transaction_monitor::{MonitoredTransaction, TransactionMonitor}; // On-Chain Graph Analyzer
