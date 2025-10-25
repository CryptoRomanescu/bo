//! Types for the sentiment analysis engine

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Source of social media data
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SocialSource {
    Twitter,
    Telegram,
    Discord,
}

impl SocialSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            SocialSource::Twitter => "twitter",
            SocialSource::Telegram => "telegram",
            SocialSource::Discord => "discord",
        }
    }
}

/// Sentiment score for a single piece of content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentimentScore {
    /// Raw sentiment value (-1.0 to 1.0, where -1 is very negative, 1 is very positive)
    pub score: f64,
    /// Confidence in the sentiment analysis (0.0 to 1.0)
    pub confidence: f64,
    /// When this sentiment was calculated
    pub timestamp: u64,
    /// Source of the sentiment data
    pub source: SocialSource,
}

/// Aggregated sentiment data for a token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSentiment {
    /// Token mint address
    pub mint: String,
    /// Aggregated sentiment scores by source
    pub scores_by_source: HashMap<SocialSource, Vec<SentimentScore>>,
    /// Overall sentiment score (weighted average)
    pub overall_score: f64,
    /// Overall confidence
    pub overall_confidence: f64,
    /// Number of mentions across all sources
    pub total_mentions: u32,
    /// Last update timestamp
    pub last_updated: u64,
}

impl TokenSentiment {
    /// Create new empty token sentiment
    pub fn new(mint: String) -> Self {
        Self {
            mint,
            scores_by_source: HashMap::new(),
            overall_score: 0.0,
            overall_confidence: 0.0,
            total_mentions: 0,
            last_updated: 0,
        }
    }

    /// Add a sentiment score from a specific source
    pub fn add_score(&mut self, score: SentimentScore, timestamp: u64) {
        let source_scores = self
            .scores_by_source
            .entry(score.source)
            .or_insert_with(Vec::new);
        source_scores.push(score);
        self.total_mentions += 1;
        self.last_updated = timestamp;
        self.recalculate_overall();
    }

    /// Recalculate overall sentiment from all sources
    fn recalculate_overall(&mut self) {
        let mut total_weighted_score = 0.0;
        let mut total_weight = 0.0;

        for scores in self.scores_by_source.values() {
            for score in scores {
                // Weight by confidence
                let weight = score.confidence;
                total_weighted_score += score.score * weight;
                total_weight += weight;
            }
        }

        if total_weight > 0.0 {
            self.overall_score = total_weighted_score / total_weight;
            self.overall_confidence = total_weight / self.total_mentions as f64;
        } else {
            self.overall_score = 0.0;
            self.overall_confidence = 0.0;
        }
    }

    /// Get sentiment score for a specific source
    pub fn get_source_score(&self, source: SocialSource) -> Option<f64> {
        self.scores_by_source.get(&source).and_then(|scores| {
            if scores.is_empty() {
                None
            } else {
                let avg = scores.iter().map(|s| s.score).sum::<f64>() / scores.len() as f64;
                Some(avg)
            }
        })
    }
}

/// Raw sentiment data from social media
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentimentData {
    /// Token mint or symbol being discussed
    pub token_identifier: String,
    /// Text content to analyze
    pub content: String,
    /// Source of the data
    pub source: SocialSource,
    /// When this content was posted
    pub timestamp: u64,
    /// Author identifier (anonymized)
    pub author_id: String,
    /// Number of likes/reactions
    pub engagement_count: u32,
}

/// Configuration for sentiment analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentimentConfig {
    /// Enable Twitter monitoring
    pub enable_twitter: bool,
    /// Enable Telegram monitoring
    pub enable_telegram: bool,
    /// Enable Discord monitoring
    pub enable_discord: bool,

    /// Twitter API credentials
    pub twitter_api_key: Option<String>,
    pub twitter_api_secret: Option<String>,
    pub twitter_bearer_token: Option<String>,

    /// Telegram bot token
    pub telegram_bot_token: Option<String>,

    /// Discord bot token
    pub discord_bot_token: Option<String>,

    /// Maximum tokens to track simultaneously
    pub max_tracked_tokens: usize,

    /// Cache TTL in seconds
    pub cache_ttl_seconds: u64,

    /// Minimum mentions required for sentiment to be considered
    pub min_mentions_threshold: u32,

    /// Rate limit: requests per minute for Twitter
    pub twitter_rate_limit_per_min: u32,

    /// Rate limit: requests per minute for Telegram
    pub telegram_rate_limit_per_min: u32,

    /// Rate limit: requests per minute for Discord
    pub discord_rate_limit_per_min: u32,

    /// Weight for sentiment in overall Oracle score (0.0 to 1.0)
    pub sentiment_weight: f64,
}

impl Default for SentimentConfig {
    fn default() -> Self {
        Self {
            enable_twitter: true,
            enable_telegram: true,
            enable_discord: true,
            twitter_api_key: None,
            twitter_api_secret: None,
            twitter_bearer_token: None,
            telegram_bot_token: None,
            discord_bot_token: None,
            max_tracked_tokens: 100,
            cache_ttl_seconds: 300, // 5 minutes
            min_mentions_threshold: 5,
            twitter_rate_limit_per_min: 300, // Twitter API v2 limit
            telegram_rate_limit_per_min: 30, // Conservative limit
            discord_rate_limit_per_min: 50,  // Discord rate limits
            sentiment_weight: 0.15,          // 15% weight in overall score
        }
    }
}

/// Metrics for sentiment analysis performance
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SentimentMetrics {
    /// Total sentiment analyses performed
    pub total_analyses: u64,
    /// Total API calls made
    pub total_api_calls: u64,
    /// Number of rate limit hits
    pub rate_limit_hits: u64,
    /// Cache hit rate
    pub cache_hit_rate: f64,
    /// Average processing time in milliseconds
    pub avg_processing_time_ms: f64,
    /// Number of errors encountered
    pub error_count: u64,
    /// Tokens currently being tracked
    pub tracked_tokens_count: usize,
}
