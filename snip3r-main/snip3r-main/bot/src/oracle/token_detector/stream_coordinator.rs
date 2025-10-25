//! Stream coordinator for managing multiple detection sources
//!
//! Coordinates multiple detection streams, handles their lifecycle,
//! and aggregates events into a single output channel.

use super::super::{DetectionEvent, DetectionSource, TokenDetectorConfig};
use super::dex_sources::{
    DetectionStream, GeyserStream, JupiterStream, PumpFunStream, PumpSwapStream, RaydiumStream,
};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tracing::{error, info, instrument, warn};

/// Manages multiple detection streams
pub struct StreamCoordinator {
    /// Configuration
    config: Arc<RwLock<TokenDetectorConfig>>,
    /// Active stream tasks
    tasks: Arc<RwLock<HashMap<DetectionSource, JoinHandle<()>>>>,
    /// Event aggregation channel sender
    event_sender: mpsc::Sender<DetectionEvent>,
    /// Internal event receiver for aggregation
    event_receiver: Option<mpsc::Receiver<DetectionEvent>>,
}

impl StreamCoordinator {
    /// Create a new stream coordinator
    pub fn new(
        config: TokenDetectorConfig,
        output_sender: mpsc::Sender<DetectionEvent>,
    ) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1000);

        Self {
            config: Arc::new(RwLock::new(config)),
            tasks: Arc::new(RwLock::new(HashMap::new())),
            event_sender: output_sender,
            event_receiver: Some(event_rx),
        }
    }

    /// Start all enabled detection streams
    #[instrument(skip(self))]
    pub async fn start_all(&mut self) -> Result<()> {
        info!("Starting all detection streams");

        let config = self.config.read().await;
        let enabled_sources = config.enabled_sources.clone();
        drop(config);

        for source in enabled_sources {
            if let Err(e) = self.start_stream(source).await {
                warn!("Failed to start stream {:?}: {}", source, e);
            }
        }

        // Start event aggregation task
        self.start_aggregation().await?;

        info!("All detection streams started");
        Ok(())
    }

    /// Start a specific detection stream
    #[instrument(skip(self))]
    async fn start_stream(&self, source: DetectionSource) -> Result<()> {
        info!("Starting detection stream for {:?}", source);

        let config = self.config.read().await;
        let (internal_tx, mut internal_rx) = mpsc::channel::<DetectionEvent>(100);

        // Create appropriate stream based on source
        let mut stream: Box<dyn DetectionStream> = match source {
            DetectionSource::PumpFun => {
                let endpoint = config
                    .websocket_endpoints
                    .get(&source)
                    .cloned()
                    .unwrap_or_else(|| "https://api.pump.fun/launches".to_string());

                let api_key = config.api_keys.get(&source).cloned();

                Box::new(PumpFunStream::new(endpoint, api_key, 250))
            }
            DetectionSource::Raydium => {
                let endpoint = config
                    .websocket_endpoints
                    .get(&source)
                    .cloned()
                    .unwrap_or_else(|| "https://api.raydium.io/pools".to_string());

                Box::new(RaydiumStream::new(endpoint, 250))
            }
            DetectionSource::Jupiter => {
                let endpoint = config
                    .websocket_endpoints
                    .get(&source)
                    .cloned()
                    .unwrap_or_else(|| "https://api.jup.ag/tokens".to_string());

                Box::new(JupiterStream::new(endpoint, 500))
            }
            DetectionSource::PumpSwap => {
                let endpoint = config
                    .websocket_endpoints
                    .get(&source)
                    .cloned()
                    .unwrap_or_else(|| "https://api.pumpswap.io/tokens".to_string());

                let api_key = config.api_keys.get(&source).cloned();

                Box::new(PumpSwapStream::new(endpoint, api_key, 250))
            }
            DetectionSource::Geyser => {
                let endpoint = config
                    .websocket_endpoints
                    .get(&source)
                    .cloned()
                    .unwrap_or_else(|| "http://localhost:10000".to_string());

                Box::new(GeyserStream::new(endpoint))
            }
            DetectionSource::WebSocket => {
                // Generic WebSocket not implemented yet
                return Err(anyhow::anyhow!("Generic WebSocket not implemented"));
            }
        };

        drop(config);

        // Spawn task to run the stream
        let task = tokio::spawn(async move {
            if let Err(e) = stream.start(internal_tx).await {
                error!("Stream {:?} error: {}", source, e);
            }
        });

        // Store task handle
        let mut tasks = self.tasks.write().await;
        tasks.insert(source, task);

        // Forward events from this stream to main event channel
        let event_sender = self.event_sender.clone();
        tokio::spawn(async move {
            while let Some(event) = internal_rx.recv().await {
                if let Err(e) = event_sender.send(event).await {
                    error!("Failed to forward event from {:?}: {}", source, e);
                    break;
                }
            }
        });

        Ok(())
    }

    /// Start event aggregation task
    async fn start_aggregation(&mut self) -> Result<()> {
        let mut event_rx = self
            .event_receiver
            .take()
            .context("Event receiver already consumed")?;

        let event_sender = self.event_sender.clone();

        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if let Err(e) = event_sender.send(event).await {
                    error!("Failed to aggregate event: {}", e);
                    break;
                }
            }
        });

        Ok(())
    }

    /// Stop all detection streams
    #[instrument(skip(self))]
    pub async fn stop_all(&self) -> Result<()> {
        info!("Stopping all detection streams");

        let mut tasks = self.tasks.write().await;

        for (source, task) in tasks.drain() {
            info!("Stopping stream {:?}", source);
            task.abort();
        }

        info!("All detection streams stopped");
        Ok(())
    }

    /// Stop a specific detection stream
    #[instrument(skip(self))]
    pub async fn stop_stream(&self, source: DetectionSource) -> Result<()> {
        info!("Stopping detection stream for {:?}", source);

        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.remove(&source) {
            task.abort();
            info!("Stream {:?} stopped", source);
        } else {
            warn!("Stream {:?} was not running", source);
        }

        Ok(())
    }

    /// Check if a stream is running
    pub async fn is_stream_running(&self, source: DetectionSource) -> bool {
        let tasks = self.tasks.read().await;
        tasks.contains_key(&source)
    }

    /// Get count of active streams
    pub async fn active_stream_count(&self) -> usize {
        let tasks = self.tasks.read().await;
        tasks.len()
    }

    /// Update configuration
    pub async fn update_config(&self, config: TokenDetectorConfig) {
        let mut current_config = self.config.write().await;
        *current_config = config;
        info!("Stream coordinator configuration updated");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::token_detector::TokenDetectorConfig;
    use std::collections::HashSet;

    #[tokio::test]
    async fn test_stream_coordinator_creation() {
        let (tx, _rx) = mpsc::channel(100);
        let config = TokenDetectorConfig::default();
        let coordinator = StreamCoordinator::new(config, tx);

        assert_eq!(coordinator.active_stream_count().await, 0);
    }

    #[tokio::test]
    async fn test_stream_coordinator_active_count() {
        let (tx, _rx) = mpsc::channel(100);
        let mut config = TokenDetectorConfig::default();
        config.enabled_sources = HashSet::new();

        let coordinator = StreamCoordinator::new(config, tx);

        // No streams should be running initially
        assert_eq!(coordinator.active_stream_count().await, 0);
    }

    #[tokio::test]
    async fn test_is_stream_running() {
        let (tx, _rx) = mpsc::channel(100);
        let mut config = TokenDetectorConfig::default();
        config.enabled_sources = HashSet::new();

        let coordinator = StreamCoordinator::new(config, tx);

        assert!(!coordinator.is_stream_running(DetectionSource::PumpFun).await);
    }
}
