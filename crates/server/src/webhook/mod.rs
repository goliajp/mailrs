pub mod global;
pub mod listener;
pub mod signer;
pub mod store;
pub mod worker;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// a webhook subscription record from the database
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Subscription {
    pub id: i64,
    #[allow(dead_code)]
    pub account_address: String,
    pub url: String,
    pub event_type: String,
    pub filter_sender: Option<String>,
    pub filter_thread_id: Option<String>,
    pub signing_secret: String,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}

/// an outbox entry for pending webhook delivery
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OutboxEntry {
    pub id: i64,
    pub subscription_id: i64,
    pub payload: serde_json::Value,
    #[allow(dead_code)]
    pub status: String,
    pub attempts: i32,
    pub max_attempts: i32,
    #[allow(dead_code)]
    pub next_retry: i64,
    #[allow(dead_code)]
    pub last_error: Option<String>,
    #[allow(dead_code)]
    pub created_at: i64,
    #[allow(dead_code)]
    pub updated_at: i64,
}

/// the webhook payload sent to subscribers
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebhookPayload {
    pub event: String,
    pub timestamp: String,
    pub data: WebhookData,
}

/// the data portion of a webhook payload
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebhookData {
    pub user: String,
    pub thread_id: String,
    pub sender: String,
    pub subject: String,
    pub snippet: String,
}
