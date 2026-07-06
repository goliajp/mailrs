//! Parameter structs for the MCP tool surface. Kept in one file so
//! `schemars::JsonSchema` derive output is deterministic and easy to
//! diff. Every field carries a doc comment — those bubble through to
//! the MCP tool JSON schema and become the AI-visible spec.

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListConversationsParams {
    /// Max threads to return. Defaults to 50, caps at 500.
    #[serde(default)]
    pub limit: Option<u32>,
    /// Folder filter (e.g. "Sent", "INBOX"). Case-insensitive.
    #[serde(default)]
    pub folder: Option<String>,
    /// Category filter (e.g. "personal", "newsletter", "notifications").
    #[serde(default)]
    pub category: Option<String>,
    /// When true, restrict to threads with any unread message.
    #[serde(default)]
    pub unread_only: Option<bool>,
    /// Pagination cursor: return threads with `last_date < before_ts`.
    #[serde(default)]
    pub before_ts: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadThreadParams {
    /// Thread ID as returned by `list_conversations`.
    pub thread_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchConversationsParams {
    /// Free-text query. Case-insensitive; matches subject +
    /// participants + snippet.
    pub q: String,
    /// Max hits (default 20, cap 100).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateAccountParams {
    /// New account address (email).
    pub address: String,
    /// Display name.
    pub display_name: String,
    /// Initial password (Argon2-hashed server-side).
    pub password: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddressParams {
    /// Account address to act on.
    pub address: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddAliasParams {
    /// Alias source address, e.g. `sales@example.com`.
    pub source_address: String,
    /// Target address the alias forwards to.
    pub target_address: String,
    /// Alias type: `alias` (default, deliver to local target) or `forward`.
    #[serde(default)]
    pub alias_type: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemoveAliasParams {
    /// Deterministic alias id returned by `list_aliases`.
    pub id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DomainNameParams {
    /// Domain name, e.g. `example.com`.
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveSignatureParams {
    /// Signature display name.
    pub name: String,
    /// HTML body of the signature.
    #[serde(default)]
    pub html: String,
    /// Plain-text fallback body.
    #[serde(default)]
    pub text_content: String,
    /// If true, make this the caller's default signature.
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SignatureIdParams {
    /// Signature id returned by `list_signatures` / `save_signature`.
    pub id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateWebhookParams {
    /// Account address that owns the subscription.
    pub account_address: String,
    /// HTTPS URL webhook events are POSTed to.
    pub url: String,
    /// Event class the webhook fires on (e.g. `mail.new`, `mail.bounce`).
    pub event_type: String,
    /// Optional: only fire when the sender address matches (glob-free).
    #[serde(default)]
    pub filter_sender: Option<String>,
    /// Optional: only fire for events on this thread id.
    #[serde(default)]
    pub filter_thread_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebhookIdParams {
    /// Webhook id returned by `list_webhooks` / `create_webhook`.
    pub id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListDraftsParams {
    /// Optional: cap output. Omit to return all.
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveDraftParams {
    /// Recipient list, comma-separated (stored as-is).
    #[serde(default)]
    pub to: String,
    /// Cc recipients, comma-separated.
    #[serde(default)]
    pub cc: String,
    /// Bcc recipients, comma-separated.
    #[serde(default)]
    pub bcc: String,
    /// Subject line.
    #[serde(default)]
    pub subject: String,
    /// Body text (plain UTF-8; agents can nest markup if the client
    /// renders it — mailrs stores verbatim).
    #[serde(default)]
    pub body: String,
    /// Optional: thread id this draft replies to.
    #[serde(default)]
    pub reply_to_thread_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DraftIdParams {
    /// Draft id returned by `list_drafts` / `save_draft`.
    pub id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveTemplateParams {
    /// Template display name.
    pub name: String,
    /// Default subject when the template is applied to a new compose.
    #[serde(default)]
    pub subject: String,
    /// HTML body.
    #[serde(default)]
    pub html_body: String,
    /// Plain-text fallback body.
    #[serde(default)]
    pub text_body: String,
    /// Optional category (any string; used for grouping in UI).
    #[serde(default)]
    pub category: String,
    /// If true, mark as the caller's default template.
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TemplateIdParams {
    /// Template id returned by `list_templates` / `save_template`.
    pub id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AuditQueryParams {
    /// Max rows (default 50).
    #[serde(default = "default_audit_mcp_limit")]
    pub limit: u32,
}

fn default_audit_mcp_limit() -> u32 {
    50
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchContactsParams {
    /// Substring to match against contact addresses / names.
    pub q: String,
    /// Max contacts to return (default 20).
    #[serde(default = "default_contacts_limit")]
    pub limit: u32,
}

fn default_contacts_limit() -> u32 {
    20
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarkThreadReadParams {
    /// Thread ID as returned by `list_conversations`.
    pub thread_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SendEmailParams {
    /// Recipient list.
    pub to: Vec<String>,
    /// Subject line.
    pub subject: String,
    /// Plain-text body. UTF-8.
    pub body: String,
    /// Optional Cc list.
    #[serde(default)]
    pub cc: Option<Vec<String>>,
    /// Optional From override — must be allowed by the caller's
    /// `send_as` permission or match their own address.
    #[serde(default)]
    pub from: Option<String>,
    /// Optional In-Reply-To Message-ID for threading.
    #[serde(default)]
    pub in_reply_to: Option<String>,
}
