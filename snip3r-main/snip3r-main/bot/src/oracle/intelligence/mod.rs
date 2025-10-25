//! Multi-Modal Intelligence Layer for Social Media Sentiment Analysis
//!
//! This module provides real-time sentiment tracking across Twitter, Telegram, and Discord
//! to enhance Oracle scoring with social media intelligence.

pub mod discord_monitor;
pub mod sentiment_cache;
pub mod sentiment_engine;
pub mod telegram_scraper;
pub mod twitter_client;
pub mod types;

// Re-export main types
pub use discord_monitor::DiscordMonitor;
pub use sentiment_cache::SentimentCache;
pub use sentiment_engine::SentimentEngine;
pub use telegram_scraper::TelegramScraper;
pub use twitter_client::TwitterClient;
pub use types::{
    SentimentConfig, SentimentData, SentimentMetrics, SentimentScore, SocialSource, TokenSentiment,
};
