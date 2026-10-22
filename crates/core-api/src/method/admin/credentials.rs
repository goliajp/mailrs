//! The machine-to-machine surface: webhook subscriptions.

use crate::types::UserAddress;
use serde::{Deserialize, Serialize};

// ════════════════════════════════════════════════════════════════════
// Wire types — webhook subscriptions
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookSubWire {
    pub id: i64,
    pub account_address: UserAddress,
    pub url: String,
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_sender: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_thread_id: Option<String>,
    pub signing_secret: String,
    pub active: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWebhookRequest {
    pub account_address: UserAddress,
    pub url: String,
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_sender: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWebhookResponse {
    pub id: i64,
    pub signing_secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookListResponse {
    pub items: Vec<WebhookSubWire>,
}

// ════════════════════════════════════════════════════════════════════
// Tests
