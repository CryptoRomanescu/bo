//! OracleActor - Actor wrapper for PredictiveOracle
//!
//! This actor wraps the PredictiveOracle component and handles scoring requests.

use super::messages::{GetOracleMetrics, OracleMetrics, ScoreCandidate, UpdateOracleConfig};
use crate::oracle::quantum_oracle::{PredictiveOracle, ScoredCandidate, SimpleOracleConfig};
use crate::types::PremintCandidate;
use actix::prelude::*;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{error, info, warn};

/// Actor that manages the PredictiveOracle component
pub struct OracleActor {
    oracle: Arc<Mutex<Option<PredictiveOracle>>>,
    config: Arc<RwLock<SimpleOracleConfig>>,
    candidate_sender: mpsc::Sender<PremintCandidate>,
    scored_receiver: Arc<Mutex<mpsc::Receiver<ScoredCandidate>>>,
    metrics: Arc<RwLock<OracleMetrics>>,
}

impl OracleActor {
    /// Create a new OracleActor
    pub fn new(
        config: SimpleOracleConfig,
        scored_sender: mpsc::Sender<ScoredCandidate>,
    ) -> Result<Self, anyhow::Error> {
        let (candidate_sender, candidate_receiver) = mpsc::channel(100);
        let (internal_scored_sender, scored_receiver) = mpsc::channel(100);

        let config_arc = Arc::new(RwLock::new(config.clone()));

        // Create the oracle
        let oracle = PredictiveOracle::new(
            candidate_receiver,
            internal_scored_sender.clone(),
            Arc::clone(&config_arc),
        )?;

        let oracle_arc = Arc::new(Mutex::new(Some(oracle)));

        // Spawn a task to forward scored candidates
        let scored_receiver_arc = Arc::new(Mutex::new(scored_receiver));
        let forward_receiver = Arc::clone(&scored_receiver_arc);
        let forward_sender = scored_sender.clone();

        tokio::spawn(async move {
            loop {
                let mut receiver = forward_receiver.lock().await;
                match receiver.recv().await {
                    Some(scored) => {
                        if let Err(e) = forward_sender.send(scored).await {
                            error!("Failed to forward scored candidate: {}", e);
                            break;
                        }
                    }
                    None => {
                        info!("Scored candidate channel closed");
                        break;
                    }
                }
            }
        });

        Ok(Self {
            oracle: oracle_arc,
            config: config_arc,
            candidate_sender,
            scored_receiver: scored_receiver_arc,
            metrics: Arc::new(RwLock::new(OracleMetrics {
                total_scored: 0,
                avg_scoring_time: 0.0,
                high_score_count: 0,
            })),
        })
    }
}

impl Actor for OracleActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("OracleActor started");

        // Start a processing loop that receives candidates and scores them
        let oracle_arc = Arc::clone(&self.oracle);
        let mut candidate_receiver = self.candidate_sender.clone();

        ctx.spawn(
            async move {
                info!("OracleActor: Starting candidate processing loop");

                // The PredictiveOracle will process candidates internally
                // through its own receiver, so we just need to keep the actor alive
            }
            .into_actor(self),
        );
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("OracleActor stopped");
    }
}

// Handle ScoreCandidate messages
impl Handler<ScoreCandidate> for OracleActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    #[tracing::instrument(skip(self, msg, _ctx), fields(mint = %msg.candidate.mint))]
    fn handle(&mut self, msg: ScoreCandidate, _ctx: &mut Context<Self>) -> Self::Result {
        let sender = self.candidate_sender.clone();
        let metrics = Arc::clone(&self.metrics);

        Box::pin(
            async move {
                // Send candidate to oracle for scoring
                sender
                    .send(msg.candidate)
                    .await
                    .map_err(|e| format!("Failed to send candidate to oracle: {}", e))?;

                // Update metrics
                let mut m = metrics.write().await;
                m.total_scored += 1;

                Ok(())
            }
            .into_actor(self),
        )
    }
}

// Handle UpdateOracleConfig messages
impl Handler<UpdateOracleConfig> for OracleActor {
    type Result = ResponseActFuture<Self, ()>;

    fn handle(&mut self, msg: UpdateOracleConfig, _ctx: &mut Context<Self>) -> Self::Result {
        let config = Arc::clone(&self.config);

        Box::pin(
            async move {
                let mut cfg = config.write().await;

                if let Some(weights) = msg.weights {
                    cfg.weights = weights;
                    info!("OracleActor: Updated feature weights");
                }

                if let Some(thresholds) = msg.thresholds {
                    cfg.thresholds = thresholds;
                    info!("OracleActor: Updated score thresholds");
                }
            }
            .into_actor(self),
        )
    }
}

// Handle GetOracleMetrics messages
impl Handler<GetOracleMetrics> for OracleActor {
    type Result = ResponseActFuture<Self, OracleMetrics>;

    fn handle(&mut self, _msg: GetOracleMetrics, _ctx: &mut Context<Self>) -> Self::Result {
        let metrics = Arc::clone(&self.metrics);

        Box::pin(async move { metrics.read().await.clone() }.into_actor(self))
    }
}
