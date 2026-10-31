//! Per-user content the UI stores: drafts, signatures, templates.

use serde::{Deserialize, Serialize};

pub const PATH_LIST_ACCOUNTS: &str = "/v1/admin/accounts";

pub const PATH_UPDATE_ACCOUNT: &str = "/v1/admin/accounts/{address}";

pub const PATH_GET_ACCOUNT_HASH: &str = "/v1/admin/accounts/{address}/credentials";

pub const PATH_SET_QUOTA: &str = "/v1/admin/accounts/{address}/quota";

/// `GET` / `PUT` — the send-as addresses granted to an account.
pub const PATH_SEND_AS: &str = "/v1/admin/accounts/{address}/send-as";

pub const PATH_UPDATE_RECOVERY_EMAIL: &str = "/v1/admin/accounts/{address}/recovery-email";

/// `POST /v1/admin/accounts/{address}/password` — re-hash & set the
/// account's password. Used by webapi's change-password / reset-password
/// / forgot-password flows so they update fastcore's embedded kevy
/// instead of the network kevy (which fastcore never reads).
pub const PATH_SET_ACCOUNT_PASSWORD: &str = "/v1/admin/accounts/{address}/password";

/// `POST /v1/admin/accounts/{address}/password:clear` — empty the hash and
/// record when and by whom; 404 for an unknown account.
pub const PATH_CLEAR_ACCOUNT_PASSWORD: &str = "/v1/admin/accounts/{address}/password:clear";

/// `POST /v1/admin/password-clear` — the operator's clearing, on the host:
/// the core empties each password and does what the web's clearing does
/// after it. Body `{"addresses": [..]}`.
pub const PATH_OPERATOR_PASSWORD_CLEAR: &str = "/v1/admin/password-clear";

/// `POST /v1/admin/password-clear:preview` — what that clearing would cut
/// off, per address.
pub const PATH_OPERATOR_PASSWORD_CLEAR_PREVIEW: &str = "/v1/admin/password-clear:preview";

/// `POST /v1/users/{user}/messages/{uid}/flags` — patch the flags
/// bitmask on a message that lives in fastcore's embedded kevy. Used
/// by webapi's update_flags / delete_message handlers.
pub const PATH_SET_MESSAGE_FLAGS: &str = "/v1/users/{user}/messages/{uid}/flags";

pub const PATH_LIST_DOMAINS: &str = "/v1/admin/domains";

pub const PATH_REMOVE_DOMAIN: &str = "/v1/admin/domains/{name}";

pub const PATH_EFFECTIVE_PERMISSIONS: &str = "/v1/admin/accounts/{address}/effective-permissions";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftWire {
    pub id: i64,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_thread_id: Option<String>,
    /// Epoch seconds.
    pub created_at: i64,
    /// Epoch seconds.
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
/// The compose autosave's body, and the Draft tab's save.
///
/// `deny_unknown_fields`: this runs every three seconds while someone is
/// typing, and a field the struct does not name was silently dropped —
/// which is how a reply reopened from the Draft tab lost the conversation
/// it belonged to. A 400 naming the field is the difference between a bug
/// found in a minute and one found in a week. The shape is pinned by
/// `wire-contract/requests/draft-save.json`, checked on both sides.
#[serde(deny_unknown_fields)]
pub struct SaveDraftRequest {
    /// When present, upsert that draft in place (keeps its id) instead of
    /// allocating a new one — so a compose session's periodic autosave
    /// updates one draft rather than spawning a new one each tick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub cc: String,
    #[serde(default)]
    pub bcc: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub reply_to_thread_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SaveDraftResponse {
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftListResponse {
    pub items: Vec<DraftWire>,
}

pub const PATH_LIST_DRAFTS: &str = "/v1/users/{user}/drafts";

pub const PATH_DELETE_DRAFT: &str = "/v1/users/{user}/drafts/{id}";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureWire {
    pub id: i64,
    pub name: String,
    pub html: String,
    pub text_content: String,
    pub is_default: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SaveSignatureRequest {
    pub name: String,
    #[serde(default)]
    pub html: String,
    #[serde(default)]
    pub text_content: String,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SaveSignatureResponse {
    pub id: i64,
}

pub const PATH_LIST_SIGNATURES: &str = "/v1/users/{user}/signatures";

pub const PATH_DELETE_SIGNATURE: &str = "/v1/users/{user}/signatures/{id}";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateWire {
    pub id: i64,
    pub name: String,
    pub subject: String,
    pub html_body: String,
    pub text_body: String,
    pub category: String,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SaveTemplateRequest {
    pub name: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub html_body: String,
    #[serde(default)]
    pub text_body: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SaveTemplateResponse {
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateListResponse {
    pub items: Vec<TemplateWire>,
}
