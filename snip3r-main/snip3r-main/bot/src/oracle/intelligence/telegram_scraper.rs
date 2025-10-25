//! Telegram scraper for monitoring group/channel sentiment
//!
//! Uses Telegram Bot API to monitor messages in groups and channels.

use super::types::{SentimentConfig, SentimentData, SocialSource};
use anyhow::{Context, Result};
use governor::{clock::DefaultClock, state::InMemoryState, state::NotKeyed, Quota, RateLimiter};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, instrument, warn};

/// Telegram scraper for sentiment monitoring
pub struct TelegramScraper {
    /// HTTP client for API requests
    http_client: Client,
    /// Telegram bot token
    bot_token: String,
    /// Rate limiter
    rate_limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    /// Configuration
    config: SentimentConfig,
    /// API call counter
    api_calls: Arc<Mutex<u64>>,
    /// Last update ID for getting new messages
    last_update_id: Arc<Mutex<Option<i64>>>,
}

/// Telegram API update response
#[derive(Debug, Deserialize)]
struct GetUpdatesResponse {
    ok: bool,
    result: Option<Vec<Update>>,
}

#[derive(Debug, Deserialize)]
struct Update {
    update_id: i64,
    message: Option<Message>,
    channel_post: Option<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    message_id: i64,
    from: Option<User>,
    chat: Chat,
    date: i64,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct User {
    id: i64,
    first_name: String,
}

#[derive(Debug, Deserialize)]
struct Chat {
    id: i64,
    #[serde(rename = "type")]
    chat_type: String,
    title: Option<String>,
}

impl TelegramScraper {
    /// Create a new Telegram scraper
    pub fn new(config: SentimentConfig) -> Result<Self> {
        let bot_token = config
            .telegram_bot_token
            .clone()
            .context("Telegram bot token not configured")?;

        // Create rate limiter based on config
        let quota = Quota::per_minute(
            NonZeroU32::new(config.telegram_rate_limit_per_min)
                .context("Invalid Telegram rate limit")?,
        );
        let rate_limiter = Arc::new(RateLimiter::direct(quota));

        Ok(Self {
            http_client: Client::new(),
            bot_token,
            rate_limiter,
            config,
            api_calls: Arc::new(Mutex::new(0)),
            last_update_id: Arc::new(Mutex::new(None)),
        })
    }

    /// Get updates from Telegram (polling mode)
    #[instrument(skip(self))]
    pub async fn get_updates(&self, timeout: u32) -> Result<Vec<SentimentData>> {
        // Wait for rate limiter
        self.rate_limiter.until_ready().await;

        // Increment API call counter
        let mut api_calls = self.api_calls.lock().await;
        *api_calls += 1;
        drop(api_calls);

        let last_id = self.last_update_id.lock().await;
        let offset = last_id.map(|id| id + 1).unwrap_or(0);
        drop(last_id);

        debug!("Fetching Telegram updates with offset: {}", offset);

        // Build API URL
        let url = format!(
            "https://api.telegram.org/bot{}/getUpdates?offset={}&timeout={}",
            self.bot_token, offset, timeout
        );

        // Make API request
        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .context("Failed to send Telegram API request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Telegram API error: {} - {}", status, body);
            return Err(anyhow::anyhow!(
                "Telegram API returned status {}: {}",
                status,
                body
            ));
        }

        let updates_response: GetUpdatesResponse = response
            .json()
            .await
            .context("Failed to parse Telegram API response")?;

        if !updates_response.ok {
            return Err(anyhow::anyhow!("Telegram API returned ok=false"));
        }

        let updates = updates_response.result.unwrap_or_default();
        info!("Received {} Telegram updates", updates.len());

        // Update last_update_id
        if let Some(last_update) = updates.last() {
            let mut last_id = self.last_update_id.lock().await;
            *last_id = Some(last_update.update_id);
        }

        // Convert updates to sentiment data
        let sentiment_data: Vec<SentimentData> = updates
            .into_iter()
            .filter_map(|update| self.update_to_sentiment_data(update))
            .collect();

        Ok(sentiment_data)
    }

    /// Search for messages containing specific keywords
    #[instrument(skip(self))]
    pub async fn search_messages(&self, keywords: Vec<String>) -> Result<Vec<SentimentData>> {
        let updates = self.get_updates(30).await?;

        // Filter messages containing any of the keywords
        let filtered: Vec<SentimentData> = updates
            .into_iter()
            .filter(|data| {
                keywords.iter().any(|keyword| {
                    data.content
                        .to_lowercase()
                        .contains(&keyword.to_lowercase())
                        || data
                            .token_identifier
                            .to_lowercase()
                            .contains(&keyword.to_lowercase())
                })
            })
            .collect();

        debug!("Filtered {} messages matching keywords", filtered.len());
        Ok(filtered)
    }

    /// Convert a Telegram update to sentiment data
    fn update_to_sentiment_data(&self, update: Update) -> Option<SentimentData> {
        // Try to get message from either direct message or channel post
        let message = update.message.or(update.channel_post)?;
        let text = message.text?;

        // Extract potential token identifier from chat title or message
        let token_identifier = message
            .chat
            .title
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        Some(SentimentData {
            token_identifier,
            content: text,
            source: SocialSource::Telegram,
            timestamp: message.date as u64,
            author_id: message
                .from
                .map(|u| u.id.to_string())
                .unwrap_or_else(|| message.chat.id.to_string()),
            engagement_count: 1, // Telegram doesn't provide this easily via bot API
        })
    }

    /// Monitor specific chat/channel for token mentions
    #[instrument(skip(self))]
    pub async fn monitor_chat(&self, token_identifiers: Vec<String>) -> Result<Vec<SentimentData>> {
        let updates = self.get_updates(30).await?;

        // Filter for relevant token mentions
        let filtered: Vec<SentimentData> = updates
            .into_iter()
            .filter(|data| {
                token_identifiers
                    .iter()
                    .any(|token| data.content.to_lowercase().contains(&token.to_lowercase()))
            })
            .collect();

        info!("Found {} relevant Telegram messages", filtered.len());
        Ok(filtered)
    }

    /// Get the number of API calls made
    pub async fn get_api_call_count(&self) -> u64 {
        *self.api_calls.lock().await
    }

    /// Check if the scraper is properly configured
    pub fn is_enabled(&self) -> bool {
        self.config.enable_telegram && !self.bot_token.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> SentimentConfig {
        SentimentConfig {
            enable_telegram: true,
            telegram_bot_token: Some("123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11".to_string()),
            telegram_rate_limit_per_min: 30,
            ..Default::default()
        }
    }

    #[test]
    fn test_telegram_scraper_creation() {
        let config = create_test_config();
        let scraper = TelegramScraper::new(config.clone());
        assert!(scraper.is_ok());

        let scraper = scraper.unwrap();
        assert!(scraper.is_enabled());
    }

    #[test]
    fn test_telegram_scraper_creation_without_token() {
        let mut config = create_test_config();
        config.telegram_bot_token = None;

        let scraper = TelegramScraper::new(config);
        assert!(scraper.is_err());
    }

    #[tokio::test]
    async fn test_api_call_counter() {
        let config = create_test_config();
        let scraper = TelegramScraper::new(config).unwrap();

        assert_eq!(scraper.get_api_call_count().await, 0);
    }

    #[test]
    fn test_is_enabled() {
        let config = create_test_config();
        let scraper = TelegramScraper::new(config).unwrap();
        assert!(scraper.is_enabled());

        let mut config = create_test_config();
        config.enable_telegram = false;
        let scraper = TelegramScraper::new(config).unwrap();
        assert!(!scraper.is_enabled());
    }

    #[test]
    fn test_update_to_sentiment_data() {
        let config = create_test_config();
        let scraper = TelegramScraper::new(config).unwrap();

        let update = Update {
            update_id: 1,
            message: Some(Message {
                message_id: 123,
                from: Some(User {
                    id: 456,
                    first_name: "Test".to_string(),
                }),
                chat: Chat {
                    id: 789,
                    chat_type: "group".to_string(),
                    title: Some("TestToken Community".to_string()),
                },
                date: 1640995200,
                text: Some("This token is amazing!".to_string()),
            }),
            channel_post: None,
        };

        let sentiment_data = scraper.update_to_sentiment_data(update);
        assert!(sentiment_data.is_some());

        let data = sentiment_data.unwrap();
        assert_eq!(data.source, SocialSource::Telegram);
        assert_eq!(data.content, "This token is amazing!");
        assert_eq!(data.token_identifier, "TestToken Community");
    }
}
