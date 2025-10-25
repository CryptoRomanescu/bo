//! Actor System Foundation Module
//!
//! This module provides an actor-based architecture for the bot components,
//! enabling better scalability, fault tolerance, and message-based communication.
//!
//! # Architecture
//!
//! The actor system consists of:
//! - **OracleActor**: Wraps PredictiveOracle for candidate scoring
//! - **StorageActor**: Wraps DecisionLedger for persistent storage
//! - **MonitorActor**: Wraps TransactionMonitor for transaction tracking
//! - **SupervisorActor**: Manages all actors with restart policies
//!
//! # Message Passing
//!
//! Actors communicate asynchronously via message passing. Each actor has
//! a mailbox that receives messages and processes them sequentially.
//!
//! # Fault Tolerance
//!
//! The SupervisorActor implements supervision strategies to automatically
//! restart crashed actors, ensuring system resilience.

pub mod messages;
pub mod monitor_actor;
pub mod oracle_actor;
pub mod storage_actor;
pub mod supervisor_actor;

// Re-export main types for convenience
pub use messages::*;
pub use monitor_actor::MonitorActor;
pub use oracle_actor::OracleActor;
pub use storage_actor::StorageActor;
pub use supervisor_actor::{SupervisionStrategy, SupervisorActor};
