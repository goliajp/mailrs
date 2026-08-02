//! The machine-to-machine surface: apps, API keys, webhooks, OAuth
//! clients, signing keys.

use crate::types::UserAddress;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppWire {
    pub id: i64,
    pub app_id: String,
    pub name: String,
    pub description: String,
    pub owner_address: UserAddress,
    pub scopes: Vec<String>,
    pub active: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAppRequest {
    pub app_id: String,
    pub name: String,
    pub description: String,
    pub owner_address: UserAddress,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAppScopesRequest {
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppListResponse {
    pub items: Vec<AppWire>,
}

// ════════════════════════════════════════════════════════════════════
// Wire types — audit log
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyWire {
    pub id: i64,
    pub prefix: String,
    /// Full key (only returned at create-time).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_key: Option<String>,
    /// Argon2 hash of the full key (server-side only).
    #[serde(skip)]
    pub key_hash: String,
    pub account_address: UserAddress,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<i64>,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApiKeyRequest {
    pub account_address: UserAddress,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    /// Optional app binding (e.g. when issued from /api/agent/keys flow).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApiKeyResponse {
    pub id: i64,
    pub prefix: String,
    /// Full key shown once.
    pub full_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyListResponse {
    pub items: Vec<ApiKeyWire>,
}

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
// Wire types — oauth / oidc provider
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClientWire {
    pub client_id: String,
    /// Hash of client secret (Argon2). Never round-trip to UI.
    #[serde(skip)]
    pub secret_hash: String,
    pub name: String,
    pub redirect_uris: Vec<String>,
    pub scopes: Vec<String>,
    pub trusted: bool,
    pub active: bool,
    pub created_by: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOAuthClientRequest {
    pub client_id: String,
    pub name: String,
    pub redirect_uris: Vec<String>,
    pub scopes: Vec<String>,
    pub trusted: bool,
    pub created_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOAuthClientResponse {
    pub client_id: String,
    /// Plaintext secret shown once.
    pub client_secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClientListResponse {
    pub items: Vec<OAuthClientWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningKeyWire {
    pub kid: String,
    pub public_key_pem: String,
    /// Private key — only for core internal use.
    #[serde(skip)]
    pub private_key_pem: String,
    pub algorithm: String,
    pub active: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningKeyListResponse {
    pub items: Vec<SigningKeyWire>,
}

// ════════════════════════════════════════════════════════════════════
// Tests
