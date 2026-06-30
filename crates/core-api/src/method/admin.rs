//! Admin endpoints — domain_store CRUD + reconcile/export/audit/system_config.
//!
//! Source: `crates/server/src/domain_store/*.rs` (57 fn across 12 files,
//! see `docs/CURRENT_STATE_FROZEN.md` §0.5).
//!
//! Stub — path constants only; req/resp types fill in subsequent loops
//! (checklist 1.6).

// ── accounts ─────────────────────────────────────────────────────────
pub const PATH_LIST_ACCOUNTS: &str = "/v1/admin/accounts";
pub const PATH_ADD_ACCOUNT: &str = "/v1/admin/accounts";
pub const PATH_GET_ACCOUNT: &str = "/v1/admin/accounts/{address}";
pub const PATH_UPDATE_ACCOUNT: &str = "/v1/admin/accounts/{address}";
pub const PATH_REMOVE_ACCOUNT: &str = "/v1/admin/accounts/{address}";
pub const PATH_GET_ACCOUNT_HASH: &str = "/v1/admin/accounts/{address}/credentials";
pub const PATH_GET_QUOTA: &str = "/v1/admin/accounts/{address}/quota";
pub const PATH_SET_QUOTA: &str = "/v1/admin/accounts/{address}/quota";
pub const PATH_UPDATE_RECOVERY_EMAIL: &str = "/v1/admin/accounts/{address}/recovery-email";

// ── aliases ──────────────────────────────────────────────────────────
pub const PATH_LIST_ALIASES: &str = "/v1/admin/aliases";
pub const PATH_ADD_ALIAS: &str = "/v1/admin/aliases";
pub const PATH_REMOVE_ALIAS: &str = "/v1/admin/aliases/{id}";
pub const PATH_RESOLVE_RECIPIENT: &str = "/v1/admin/aliases:resolve-recipient";

// ── apps ─────────────────────────────────────────────────────────────
pub const PATH_LIST_APPS: &str = "/v1/admin/apps";
pub const PATH_CREATE_APP: &str = "/v1/admin/apps";
pub const PATH_GET_APP: &str = "/v1/admin/apps/{app_id}";
pub const PATH_GET_APP_BY_ID: &str = "/v1/admin/apps/by-id/{id}";
pub const PATH_REMOVE_APP: &str = "/v1/admin/apps/{app_id}";
pub const PATH_UPDATE_APP_SCOPES: &str = "/v1/admin/apps/{app_id}/scopes";

// ── audit log ────────────────────────────────────────────────────────
pub const PATH_LOG_AUDIT: &str = "/v1/admin/audit-log";
pub const PATH_LIST_AUDIT_LOG: &str = "/v1/admin/audit-log";
pub const PATH_CLEANUP_AUDIT: &str = "/v1/admin/audit-log:cleanup";

// ── domains ──────────────────────────────────────────────────────────
pub const PATH_LIST_DOMAINS: &str = "/v1/admin/domains";
pub const PATH_ADD_DOMAIN: &str = "/v1/admin/domains";
pub const PATH_REMOVE_DOMAIN: &str = "/v1/admin/domains/{name}";

// ── email groups ─────────────────────────────────────────────────────
pub const PATH_LIST_EMAIL_GROUPS: &str = "/v1/admin/email-groups";
pub const PATH_CREATE_EMAIL_GROUP: &str = "/v1/admin/email-groups";
pub const PATH_REMOVE_EMAIL_GROUP: &str = "/v1/admin/email-groups/{id}";
pub const PATH_LIST_EMAIL_GROUP_MEMBERS: &str = "/v1/admin/email-groups/{id}/members";
pub const PATH_ADD_EMAIL_GROUP_MEMBER: &str = "/v1/admin/email-groups/{id}/members";
pub const PATH_REMOVE_EMAIL_GROUP_MEMBER: &str = "/v1/admin/email-groups/{id}/members/{address}";

// ── encryption keys ──────────────────────────────────────────────────
pub const PATH_GET_ENCRYPTION_KEY: &str = "/v1/admin/encryption-keys/{address}/{key_type}";
pub const PATH_SET_ENCRYPTION_KEY: &str = "/v1/admin/encryption-keys/{address}/{key_type}";
pub const PATH_DELETE_ENCRYPTION_KEY: &str = "/v1/admin/encryption-keys/{address}/{key_type}";
pub const PATH_LIST_ENCRYPTION_KEYS: &str = "/v1/admin/encryption-keys/{address}";

// ── groups + permissions ─────────────────────────────────────────────
pub const PATH_LIST_GROUPS: &str = "/v1/admin/groups";
pub const PATH_GET_GROUP_PERMISSIONS: &str = "/v1/admin/groups/{id}/permissions";
pub const PATH_SET_GROUP_PERMISSIONS: &str = "/v1/admin/groups/{id}/permissions";
pub const PATH_ADD_GROUP: &str = "/v1/admin/groups";
pub const PATH_REMOVE_GROUP: &str = "/v1/admin/groups/{id}";
pub const PATH_LIST_GROUP_MEMBERS: &str = "/v1/admin/groups/{id}/members";
pub const PATH_ADD_ACCOUNT_TO_GROUP: &str = "/v1/admin/groups/{id}/members";
pub const PATH_REMOVE_ACCOUNT_FROM_GROUP: &str = "/v1/admin/groups/{id}/members/{address}";
pub const PATH_GET_ACCOUNT_GROUPS: &str = "/v1/admin/accounts/{address}/groups";
pub const PATH_GET_ACCOUNT_OVERRIDES: &str = "/v1/admin/accounts/{address}/overrides";
pub const PATH_SET_ACCOUNT_OVERRIDES: &str = "/v1/admin/accounts/{address}/overrides";
pub const PATH_EFFECTIVE_PERMISSIONS: &str = "/v1/admin/accounts/{address}/effective-permissions";
pub const PATH_INVALIDATE_PERMISSIONS: &str = "/v1/admin/permissions:invalidate";

// ── sieve ────────────────────────────────────────────────────────────
pub const PATH_GET_SIEVE: &str = "/v1/admin/accounts/{address}/sieve";
pub const PATH_SET_SIEVE: &str = "/v1/admin/accounts/{address}/sieve";
pub const PATH_DELETE_SIEVE: &str = "/v1/admin/accounts/{address}/sieve";

// ── totp ─────────────────────────────────────────────────────────────
pub const PATH_GET_TOTP: &str = "/v1/admin/accounts/{address}/totp";
pub const PATH_SAVE_TOTP: &str = "/v1/admin/accounts/{address}/totp";
pub const PATH_ENABLE_TOTP: &str = "/v1/admin/accounts/{address}/totp:enable";
pub const PATH_DISABLE_TOTP: &str = "/v1/admin/accounts/{address}/totp";
pub const PATH_CONSUME_RECOVERY_CODE: &str =
    "/v1/admin/accounts/{address}/totp:consume-recovery-code";

// ── vacation dedup ───────────────────────────────────────────────────
pub const PATH_SHOULD_SEND_VACATION: &str = "/v1/admin/vacation-dedup:should-send";
pub const PATH_RECORD_VACATION_REPLY: &str = "/v1/admin/vacation-dedup";

// ── system config ────────────────────────────────────────────────────
pub const PATH_LIST_SYSTEM_CONFIG: &str = "/v1/admin/system-config";
pub const PATH_UPDATE_SYSTEM_CONFIG: &str = "/v1/admin/system-config/{key}";
pub const PATH_DELETE_SYSTEM_CONFIG: &str = "/v1/admin/system-config/{key}";

// ── reconcile + backfill + export ────────────────────────────────────
pub const PATH_RECONCILE_MAILDIR: &str = "/v1/admin/reconcile";
pub const PATH_BACKFILL_THREADING: &str = "/v1/admin/backfill-threading";
pub const PATH_EXPORT_MESSAGES: &str = "/v1/admin/export";

// ── api keys (consumed by webapi auth) ───────────────────────────────
pub const PATH_CREATE_API_KEY: &str = "/v1/admin/api-keys";
pub const PATH_LIST_API_KEYS: &str = "/v1/admin/accounts/{address}/api-keys";
pub const PATH_REVOKE_API_KEY: &str = "/v1/admin/api-keys/{id}";
pub const PATH_GET_API_KEY_BY_PREFIX: &str = "/v1/admin/api-keys/by-prefix/{prefix}";
pub const PATH_TOUCH_API_KEY: &str = "/v1/admin/api-keys/{id}/touch";

// ── webhook subscriptions ────────────────────────────────────────────
pub const PATH_CREATE_WEBHOOK: &str = "/v1/admin/webhook-subscriptions";
pub const PATH_LIST_WEBHOOKS: &str = "/v1/admin/accounts/{address}/webhook-subscriptions";
pub const PATH_DELETE_WEBHOOK: &str = "/v1/admin/webhook-subscriptions/{id}";

// ── oauth (oidc provider) ────────────────────────────────────────────
pub const PATH_LIST_OAUTH_CLIENTS: &str = "/v1/admin/oauth-clients";
pub const PATH_CREATE_OAUTH_CLIENT: &str = "/v1/admin/oauth-clients";
pub const PATH_GET_OAUTH_CLIENT: &str = "/v1/admin/oauth-clients/{client_id}";
pub const PATH_DELETE_OAUTH_CLIENT: &str = "/v1/admin/oauth-clients/{client_id}";
pub const PATH_LIST_SIGNING_KEYS: &str = "/v1/admin/oauth-signing-keys";
pub const PATH_OAUTH_AUTH_CODE: &str = "/v1/admin/oauth-auth-codes";
pub const PATH_OAUTH_REFRESH_TOKEN: &str = "/v1/admin/oauth-refresh-tokens";
