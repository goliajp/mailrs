pub mod signer;
pub mod store;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// a webhook subscription record from the database
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Subscription {
    pub id: i64,
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
    pub status: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub next_retry: i64,
    pub last_error: Option<String>,
    pub created_at: i64,
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
