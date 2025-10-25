//! H-5N1P3R - High-performance Solana trading oracle system
//!
//! This crate provides a universe-class predictive oracle system for Solana token analysis
//! with operational memory (DecisionLedger) capabilities.

pub mod actors;
pub mod http_rate_limit;
pub mod observability;
pub mod oracle;
pub mod orchestrator;
pub mod security;
pub mod types;

// Re-export main types for convenience
pub use actors::{MonitorActor, OracleActor, StorageActor, SupervisionStrategy, SupervisorActor};
pub use orchestrator::{
    ChannelFactory, OrchestratorConfig, ShutdownCoordinator, SystemInitializer,
};
pub use types::{PremintCandidate, QuantumCandidateGui};
