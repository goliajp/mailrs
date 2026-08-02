//! Parameters for the admin tools: accounts, domains, aliases,
//! groups, apps, webhooks, the queue, audit and system config.

use serde::Deserialize;

// --- parameter structs ---
use super::*;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct CreateAccountParams {
    /// email address (e.g. "user@golia.jp")
    pub address: String,
    /// domain name (e.g. "golia.jp")
    pub domain: String,
    /// display name
    #[serde(default)]
    pub display_name: String,
    /// password (will be argon2-hashed)
    #[serde(default)]
    pub password: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RemoveAccountParams {
    /// email address to remove
    pub address: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListAccountsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SetAccountPasswordParams {
    /// email address
    pub address: String,
    /// new password (will be argon2-hashed)
    pub password: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListGroupsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct AddAccountToGroupParams {
    /// email address
    pub address: String,
    /// group ID
    pub group_id: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RemoveAccountFromGroupParams {
    /// email address
    pub address: String,
    /// group ID
    pub group_id: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetAccountPermissionsParams {
    /// email address
    pub address: String,
}

// --- domain management ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListDomainsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct AddDomainParams {
    /// domain name (e.g. "example.com")
    pub name: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RemoveDomainParams {
    /// domain name to remove
    pub name: String,
}

// --- alias management ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListAliasesAdminParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct AddAliasParams {
    /// source address (the alias, e.g. "team@golia.jp")
    pub source_address: String,
    /// target address (receives mail, e.g. "user@golia.jp")
    pub target_address: String,
    /// domain name
    pub domain: String,
    /// "alias" (local delivery) or "forward" (remote forward)
    #[serde(default = "default_alias_type")]
    pub alias_type: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RemoveAliasParams {
    /// alias ID from list_aliases
    pub id: i64,
}

// --- greylist local lists (Phase 2) ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListGreylistLocalParams {
    /// optional filter: "domain", "email", or "cidr"
    #[serde(default)]
    pub kind: Option<String>,
    /// optional filter: "white" or "black"
    #[serde(default)]
    pub list: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GreylistLocalAddParams {
    /// rule kind: "domain" (e.g. "example.com"), "email" (e.g. "user@example.com"),
    /// or "cidr" (e.g. "10.0.0.0/8" or "2001:db8::/32")
    pub kind: String,
    /// which list: "white" (skip greylist) or "black" (reject 550)
    pub list: String,
    /// the value to match, in the form implied by `kind`
    pub value: String,
    /// optional human-readable note for the audit trail
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GreylistLocalRemoveParams {
    /// id from greylist_local_list output
    pub id: i64,
}

// --- app management ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListAppsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct CreateAppParams {
    /// app display name
    pub name: String,
    /// app description
    #[serde(default)]
    pub description: String,
    /// comma-separated permission scopes
    pub scopes: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DeleteAppParams {
    /// app_id (UUID) from list_apps
    pub app_id: String,
}

// --- webhook management ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListWebhooksParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct CreateWebhookParams {
    /// callback URL (must be https, or http for localhost)
    pub url: String,
    /// event type (default: "new_message")
    #[serde(default = "default_event_type")]
    pub event_type: String,
    /// optional: only trigger for emails from this sender
    #[serde(default)]
    pub filter_sender: Option<String>,
    /// optional: only trigger for this thread
    #[serde(default)]
    pub filter_thread_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DeleteWebhookParams {
    /// webhook ID from list_webhooks
    pub id: i64,
}

// --- mail operations ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListAdminQueueParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ReconcileMaildirParams {
    /// report the maildir/PG gap without writing anything (default false)
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RetryQueueMessageParams {
    /// queue message ID
    pub id: i64,
}

// --- email group management ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListEmailGroupsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct CreateEmailGroupParams {
    /// group email address (e.g. "team@golia.jp")
    pub address: String,
    /// domain name (e.g. "golia.jp")
    pub domain: String,
    /// display name for the group
    #[serde(default)]
    pub name: String,
    /// group description
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct DeleteEmailGroupParams {
    /// email group ID from list_email_groups
    pub id: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetEmailGroupMembersParams {
    /// email group ID
    pub id: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct AddEmailGroupMemberParams {
    /// email group ID
    pub group_id: i64,
    /// account address to add
    pub address: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RemoveEmailGroupMemberParams {
    /// email group ID
    pub group_id: i64,
    /// account address to remove
    pub address: String,
}

// --- scheduled send ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetAuditLogParams {
    /// max entries to return (default 50)
    #[serde(default = "default_audit_limit")]
    pub limit: u32,
}

fn default_audit_limit() -> u32 {
    50
}

// --- signature management ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetSystemConfigParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SetSystemConfigParams {
    /// config key (e.g. "webhook_url", "ai_analysis_enabled")
    pub key: String,
    /// new value as string
    pub value: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ResetSystemConfigParams {
    /// config key to reset to default
    pub key: String,
}

// --- mail audit ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct AuditListConversationsParams {
    /// target user email address to audit
    pub target_user: String,
    /// max results (default 20, max 50)
    #[serde(default)]
    pub limit: Option<u32>,
    /// filter by category
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct AuditReadThreadParams {
    /// target user email address to audit
    pub target_user: String,
    /// thread ID to read
    pub thread_id: String,
}
