//! Twitter API client for streaming sentiment data
//!
//! Provides real-time monitoring of Twitter mentions for tokens.

use super::types::{SentimentConfig, SentimentData, SocialSource};
use anyhow::{Context, Result};
use governor::{clock::DefaultClock, state::InMemoryState, state::NotKeyed, Quota, RateLimiter};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, instrument, warn};

/// Twitter API client for sentiment monitoring
pub struct TwitterClient {
    /// HTTP client for API requests
    http_client: Client,
    /// Twitter bearer token
    bearer_token: String,
    /// Rate limiter
    rate_limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    /// Configuration
    config: SentimentConfig,
    /// API call counter
    api_calls: Arc<Mutex<u64>>,
}

/// Twitter API v2 tweet search response
#[derive(Debug, Deserialize)]
struct TwitterSearchResponse {
    data: Option<Vec<Tweet>>,
    meta: SearchMeta,
}

#[derive(Debug, Deserialize)]
struct Tweet {
    id: String,
    text: String,
    author_id: Option<String>,
    created_at: Option<String>,
    public_metrics: Option<PublicMetrics>,
}

#[derive(Debug, Deserialize)]
struct PublicMetrics {
    like_count: Option<u32>,
    retweet_count: Option<u32>,
    reply_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct SearchMeta {
    result_count: Option<usize>,
}

impl TwitterClient {
    /// Create a new Twitter client
    pub fn new(config: SentimentConfig) -> Result<Self> {
        let bearer_token = config
            .twitter_bearer_token
            .clone()
            .context("Twitter bearer token not configured")?;

        // Create rate limiter based on config
        let quota = Quota::per_minute(
            NonZeroU32::new(config.twitter_rate_limit_per_min)
                .context("Invalid Twitter rate limit")?,
        );
        let rate_limiter = Arc::new(RateLimiter::direct(quota));

        Ok(Self {
            http_client: Client::new(),
            bearer_token,
            rate_limiter,
            config,
            api_calls: Arc::new(Mutex::new(0)),
        })
    }

    /// Search for tweets mentioning a token
    #[instrument(skip(self), fields(query = %query))]
    pub async fn search_tweets(&self, query: &str, max_results: u32) -> Result<Vec<SentimentData>> {
        // Wait for rate limiter
        self.rate_limiter.until_ready().await;

        // Increment API call counter
        let mut api_calls = self.api_calls.lock().await;
        *api_calls += 1;
        drop(api_calls);

        debug!("Searching Twitter for: {}", query);

        // Build search URL (Twitter API v2)
        let url = format!(
            "https://api.twitter.com/2/tweets/search/recent?query={}&max_results={}&tweet.fields=created_at,author_id,public_metrics",
            urlencoding::encode(query),
            max_results.min(100) // Twitter API limit
        );

        // Make API request
        let response = self
            .http_client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.bearer_token))
            .send()
            .await
            .context("Failed to send Twitter API request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Twitter API error: {} - {}", status, body);
            return Err(anyhow::anyhow!(
                "Twitter API returned status {}: {}",
                status,
                body
            ));
        }

        let search_response: TwitterSearchResponse = response
            .json()
            .await
            .context("Failed to parse Twitter API response")?;

        let tweets = search_response.data.unwrap_or_default();
        info!("Found {} tweets for query: {}", tweets.len(), query);

        // Convert tweets to sentiment data
        let sentiment_data: Vec<SentimentData> = tweets
            .into_iter()
            .map(|tweet| self.tweet_to_sentiment_data(tweet, query))
            .collect();

        Ok(sentiment_data)
    }

    /// Stream tweets in real-time (simplified version using polling)
    /// Note: Real streaming would require Twitter's streaming API endpoint
    #[instrument(skip(self))]
    pub async fn stream_tweets(&self, queries: Vec<String>) -> Result<Vec<SentimentData>> {
        let mut all_data = Vec::new();

        for query in queries {
            match self.search_tweets(&query, 10).await {
                Ok(mut data) => all_data.append(&mut data),
                Err(e) => {
                    warn!("Failed to fetch tweets for {}: {}", query, e);
                }
            }
        }

        Ok(all_data)
    }

    /// Convert a tweet to sentiment data
    fn tweet_to_sentiment_data(&self, tweet: Tweet, token_identifier: &str) -> SentimentData {
        let timestamp = tweet
            .created_at
            .and_then(|dt| chrono::DateTime::parse_from_rfc3339(&dt).ok())
            .map(|dt| dt.timestamp() as u64)
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            });

        let engagement_count = tweet
            .public_metrics
            .map(|pm| {
                pm.like_count.unwrap_or(0) +
                pm.retweet_count.unwrap_or(0) * 2 + // Weight retweets higher
                pm.reply_count.unwrap_or(0)
            })
            .unwrap_or(0);

        SentimentData {
            token_identifier: token_identifier.to_string(),
            content: tweet.text,
            source: SocialSource::Twitter,
            timestamp,
            author_id: tweet.author_id.unwrap_or_else(|| tweet.id.clone()),
            engagement_count,
        }
    }

    /// Get the number of API calls made
    pub async fn get_api_call_count(&self) -> u64 {
        *self.api_calls.lock().await
    }

    /// Check if the client is properly configured
    pub fn is_enabled(&self) -> bool {
        self.config.enable_twitter && !self.bearer_token.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> SentimentConfig {
        SentimentConfig {
            enable_twitter: true,
            twitter_bearer_token: Some("test_bearer_token".to_string()),
            twitter_rate_limit_per_min: 300,
            ..Default::default()
        }
    }

    #[test]
    fn test_twitter_client_creation() {
        let config = create_test_config();
        let client = TwitterClient::new(config.clone());
        assert!(client.is_ok());

        let client = client.unwrap();
        assert!(client.is_enabled());
        assert_eq!(client.bearer_token, "test_bearer_token");
    }

    #[test]
    fn test_twitter_client_creation_without_token() {
        let mut config = create_test_config();
        config.twitter_bearer_token = None;

        let client = TwitterClient::new(config);
        assert!(client.is_err());
    }

    #[tokio::test]
    async fn test_api_call_counter() {
        let config = create_test_config();
        let client = TwitterClient::new(config).unwrap();

        assert_eq!(client.get_api_call_count().await, 0);
    }

    #[test]
    fn test_is_enabled() {
        let config = create_test_config();
        let client = TwitterClient::new(config).unwrap();
        assert!(client.is_enabled());

        let mut config = create_test_config();
        config.enable_twitter = false;
        let client = TwitterClient::new(config).unwrap();
        assert!(!client.is_enabled());
    }
}
