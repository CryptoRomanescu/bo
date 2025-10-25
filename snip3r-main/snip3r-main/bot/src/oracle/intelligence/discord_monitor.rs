//! Discord monitor for tracking server sentiment
//!
//! Monitors Discord servers for token-related discussions.

use super::types::{SentimentConfig, SentimentData, SocialSource};
use anyhow::{Context, Result};
use governor::{clock::DefaultClock, state::InMemoryState, state::NotKeyed, Quota, RateLimiter};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, instrument, warn};

/// Discord monitor for sentiment tracking
pub struct DiscordMonitor {
    /// HTTP client for API requests
    http_client: Client,
    /// Discord bot token
    bot_token: String,
    /// Rate limiter
    rate_limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    /// Configuration
    config: SentimentConfig,
    /// API call counter
    api_calls: Arc<Mutex<u64>>,
    /// Tracked channels
    tracked_channels: Arc<Mutex<Vec<String>>>,
}

/// Discord API message object
#[derive(Debug, Deserialize, Serialize)]
struct DiscordMessage {
    id: String,
    channel_id: String,
    author: DiscordUser,
    content: String,
    timestamp: String,
    #[serde(default)]
    reactions: Vec<DiscordReaction>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DiscordUser {
    id: String,
    username: String,
    discriminator: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct DiscordReaction {
    count: u32,
    emoji: DiscordEmoji,
}

#[derive(Debug, Deserialize, Serialize)]
struct DiscordEmoji {
    id: Option<String>,
    name: String,
}

impl DiscordMonitor {
    /// Create a new Discord monitor
    pub fn new(config: SentimentConfig) -> Result<Self> {
        let bot_token = config
            .discord_bot_token
            .clone()
            .context("Discord bot token not configured")?;

        // Create rate limiter based on config
        let quota = Quota::per_minute(
            NonZeroU32::new(config.discord_rate_limit_per_min)
                .context("Invalid Discord rate limit")?,
        );
        let rate_limiter = Arc::new(RateLimiter::direct(quota));

        Ok(Self {
            http_client: Client::new(),
            bot_token,
            rate_limiter,
            config,
            api_calls: Arc::new(Mutex::new(0)),
            tracked_channels: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Add a channel to track
    pub async fn track_channel(&self, channel_id: String) {
        let mut channels = self.tracked_channels.lock().await;
        if !channels.contains(&channel_id) {
            channels.push(channel_id.clone());
            info!("Started tracking Discord channel: {}", channel_id);
        }
    }

    /// Remove a channel from tracking
    pub async fn untrack_channel(&self, channel_id: &str) {
        let mut channels = self.tracked_channels.lock().await;
        channels.retain(|id| id != channel_id);
        info!("Stopped tracking Discord channel: {}", channel_id);
    }

    /// Get recent messages from a channel
    #[instrument(skip(self))]
    pub async fn get_channel_messages(
        &self,
        channel_id: &str,
        limit: u32,
    ) -> Result<Vec<SentimentData>> {
        // Wait for rate limiter
        self.rate_limiter.until_ready().await;

        // Increment API call counter
        let mut api_calls = self.api_calls.lock().await;
        *api_calls += 1;
        drop(api_calls);

        debug!("Fetching Discord messages from channel: {}", channel_id);

        // Build API URL
        let url = format!(
            "https://discord.com/api/v10/channels/{}/messages?limit={}",
            channel_id,
            limit.min(100) // Discord API limit
        );

        // Make API request
        let response = self
            .http_client
            .get(&url)
            .header("Authorization", format!("Bot {}", self.bot_token))
            .header("User-Agent", "H-5N1P3R-SentimentBot/1.0")
            .send()
            .await
            .context("Failed to send Discord API request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Discord API error: {} - {}", status, body);
            return Err(anyhow::anyhow!(
                "Discord API returned status {}: {}",
                status,
                body
            ));
        }

        let messages: Vec<DiscordMessage> = response
            .json()
            .await
            .context("Failed to parse Discord API response")?;

        info!(
            "Retrieved {} Discord messages from channel {}",
            messages.len(),
            channel_id
        );

        // Convert messages to sentiment data
        let sentiment_data: Vec<SentimentData> = messages
            .into_iter()
            .map(|msg| self.message_to_sentiment_data(msg))
            .collect();

        Ok(sentiment_data)
    }

    /// Monitor all tracked channels for token mentions
    #[instrument(skip(self))]
    pub async fn monitor_tracked_channels(
        &self,
        token_identifiers: Vec<String>,
    ) -> Result<Vec<SentimentData>> {
        let channels = self.tracked_channels.lock().await.clone();

        if channels.is_empty() {
            debug!("No Discord channels being tracked");
            return Ok(Vec::new());
        }

        let mut all_data = Vec::new();

        for channel_id in channels {
            match self.get_channel_messages(&channel_id, 50).await {
                Ok(messages) => {
                    // Filter for relevant token mentions
                    let filtered: Vec<SentimentData> = messages
                        .into_iter()
                        .filter(|data| {
                            token_identifiers.iter().any(|token| {
                                data.content.to_lowercase().contains(&token.to_lowercase())
                            })
                        })
                        .collect();

                    all_data.extend(filtered);
                }
                Err(e) => {
                    warn!(
                        "Failed to fetch messages from channel {}: {}",
                        channel_id, e
                    );
                }
            }
        }

        info!(
            "Found {} relevant Discord messages across all channels",
            all_data.len()
        );
        Ok(all_data)
    }

    /// Search for messages containing keywords
    #[instrument(skip(self))]
    pub async fn search_messages(
        &self,
        channel_id: &str,
        keywords: Vec<String>,
    ) -> Result<Vec<SentimentData>> {
        let messages = self.get_channel_messages(channel_id, 100).await?;

        // Filter messages containing any of the keywords
        let filtered: Vec<SentimentData> = messages
            .into_iter()
            .filter(|data| {
                keywords.iter().any(|keyword| {
                    data.content
                        .to_lowercase()
                        .contains(&keyword.to_lowercase())
                })
            })
            .collect();

        debug!(
            "Filtered {} Discord messages matching keywords",
            filtered.len()
        );
        Ok(filtered)
    }

    /// Convert a Discord message to sentiment data
    fn message_to_sentiment_data(&self, message: DiscordMessage) -> SentimentData {
        let timestamp = chrono::DateTime::parse_from_rfc3339(&message.timestamp)
            .ok()
            .map(|dt| dt.timestamp() as u64)
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            });

        let engagement_count: u32 = message.reactions.iter().map(|r| r.count).sum();

        // Extract potential token identifier from message content
        let token_identifier = self.extract_token_identifier(&message.content);

        SentimentData {
            token_identifier,
            content: message.content,
            source: SocialSource::Discord,
            timestamp,
            author_id: message.author.id,
            engagement_count,
        }
    }

    /// Extract token identifier from message content (basic heuristic)
    fn extract_token_identifier(&self, content: &str) -> String {
        // Look for common patterns like $TOKEN or TOKEN followed by CA/contract
        // This is a simple heuristic - in production, you'd want more sophisticated parsing
        let words: Vec<&str> = content.split_whitespace().collect();

        for word in words {
            if word.starts_with('$') && word.len() > 1 {
                return word[1..].to_string();
            }
        }

        "unknown".to_string()
    }

    /// Get the number of API calls made
    pub async fn get_api_call_count(&self) -> u64 {
        *self.api_calls.lock().await
    }

    /// Get the list of tracked channels
    pub async fn get_tracked_channels(&self) -> Vec<String> {
        self.tracked_channels.lock().await.clone()
    }

    /// Check if the monitor is properly configured
    pub fn is_enabled(&self) -> bool {
        self.config.enable_discord && !self.bot_token.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> SentimentConfig {
        SentimentConfig {
            enable_discord: true,
            discord_bot_token: Some("test_discord_bot_token".to_string()),
            discord_rate_limit_per_min: 50,
            ..Default::default()
        }
    }

    #[test]
    fn test_discord_monitor_creation() {
        let config = create_test_config();
        let monitor = DiscordMonitor::new(config.clone());
        assert!(monitor.is_ok());

        let monitor = monitor.unwrap();
        assert!(monitor.is_enabled());
    }

    #[test]
    fn test_discord_monitor_creation_without_token() {
        let mut config = create_test_config();
        config.discord_bot_token = None;

        let monitor = DiscordMonitor::new(config);
        assert!(monitor.is_err());
    }

    #[tokio::test]
    async fn test_track_channel() {
        let config = create_test_config();
        let monitor = DiscordMonitor::new(config).unwrap();

        assert_eq!(monitor.get_tracked_channels().await.len(), 0);

        monitor.track_channel("123456789".to_string()).await;
        assert_eq!(monitor.get_tracked_channels().await.len(), 1);

        // Adding same channel again shouldn't duplicate
        monitor.track_channel("123456789".to_string()).await;
        assert_eq!(monitor.get_tracked_channels().await.len(), 1);
    }

    #[tokio::test]
    async fn test_untrack_channel() {
        let config = create_test_config();
        let monitor = DiscordMonitor::new(config).unwrap();

        monitor.track_channel("123456789".to_string()).await;
        monitor.track_channel("987654321".to_string()).await;
        assert_eq!(monitor.get_tracked_channels().await.len(), 2);

        monitor.untrack_channel("123456789").await;
        assert_eq!(monitor.get_tracked_channels().await.len(), 1);

        let channels = monitor.get_tracked_channels().await;
        assert!(!channels.contains(&"123456789".to_string()));
        assert!(channels.contains(&"987654321".to_string()));
    }

    #[tokio::test]
    async fn test_api_call_counter() {
        let config = create_test_config();
        let monitor = DiscordMonitor::new(config).unwrap();

        assert_eq!(monitor.get_api_call_count().await, 0);
    }

    #[test]
    fn test_is_enabled() {
        let config = create_test_config();
        let monitor = DiscordMonitor::new(config).unwrap();
        assert!(monitor.is_enabled());

        let mut config = create_test_config();
        config.enable_discord = false;
        let monitor = DiscordMonitor::new(config).unwrap();
        assert!(!monitor.is_enabled());
    }

    #[test]
    fn test_extract_token_identifier() {
        let config = create_test_config();
        let monitor = DiscordMonitor::new(config).unwrap();

        let token = monitor.extract_token_identifier("Check out $BONK it's amazing!");
        assert_eq!(token, "BONK");

        let token = monitor.extract_token_identifier("I love $SOL");
        assert_eq!(token, "SOL");

        let token = monitor.extract_token_identifier("No token here");
        assert_eq!(token, "unknown");
    }
}
