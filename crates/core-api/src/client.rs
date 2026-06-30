//! Async HTTP client for the mailrs-core-api wire surface.
//!
//! Built on `reqwest`. webapi / sender import this with the `client`
//! feature on Cargo.toml. One instance per process, clonable via Arc.

use crate::error::{ApiResult, CoreApiError};
use crate::method;
use crate::types;

/// HTTP client wrapping a single `mailrs-core` target.
#[derive(Clone)]
pub struct Client {
    inner: reqwest::Client,
    base_url: String,
    auth_bearer: String,
}

impl Client {
    /// Build a new client.
    pub fn new(base_url: impl Into<String>, auth_bearer: impl Into<String>) -> Self {
        let inner = reqwest::Client::builder()
            .user_agent(concat!("mailrs-core-api/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("reqwest client build");
        Self {
            inner,
            base_url: base_url.into(),
            auth_bearer: auth_bearer.into(),
        }
    }

    // ── plumbing ────────────────────────────────────────────────────

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn map_status<T: serde::de::DeserializeOwned>(
        resp: reqwest::Response,
        context: &'static str,
    ) -> ApiResult<T> {
        let status = resp.status().as_u16();
        match status {
            200..=299 => resp
                .json::<T>()
                .await
                .map_err(|e| CoreApiError::Internal(format!("{context} decode: {e}"))),
            401 => Err(CoreApiError::Unauthorized),
            403 => Err(CoreApiError::Forbidden),
            404 => Err(CoreApiError::NotFound(context.into())),
            409 => Err(CoreApiError::Conflict(context.into())),
            501 => Err(CoreApiError::BackendUnsupported),
            503 => Err(CoreApiError::PoolExhausted),
            504 => Err(CoreApiError::Timeout),
            other => Err(CoreApiError::Internal(format!(
                "{context} returned {other}"
            ))),
        }
    }

    async fn map_status_unit(resp: reqwest::Response, context: &'static str) -> ApiResult<()> {
        let status = resp.status().as_u16();
        match status {
            200..=299 => Ok(()),
            401 => Err(CoreApiError::Unauthorized),
            403 => Err(CoreApiError::Forbidden),
            404 => Err(CoreApiError::NotFound(context.into())),
            409 => Err(CoreApiError::Conflict(context.into())),
            501 => Err(CoreApiError::BackendUnsupported),
            503 => Err(CoreApiError::PoolExhausted),
            504 => Err(CoreApiError::Timeout),
            other => Err(CoreApiError::Internal(format!(
                "{context} returned {other}"
            ))),
        }
    }

    async fn get_authed<T: serde::de::DeserializeOwned>(
        &self,
        path: String,
        context: &'static str,
    ) -> ApiResult<T> {
        let resp = self
            .inner
            .get(self.url(&path))
            .bearer_auth(&self.auth_bearer)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("{context} transport: {e}")))?;
        Self::map_status(resp, context).await
    }

    async fn post_authed_json<R: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: String,
        body: &R,
        context: &'static str,
    ) -> ApiResult<T> {
        let resp = self
            .inner
            .post(self.url(&path))
            .bearer_auth(&self.auth_bearer)
            .json(body)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("{context} transport: {e}")))?;
        Self::map_status(resp, context).await
    }

    async fn post_authed_no_body<T: serde::de::DeserializeOwned>(
        &self,
        path: String,
        context: &'static str,
    ) -> ApiResult<T> {
        let resp = self
            .inner
            .post(self.url(&path))
            .bearer_auth(&self.auth_bearer)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("{context} transport: {e}")))?;
        Self::map_status(resp, context).await
    }

    async fn put_authed_json<R: serde::Serialize>(
        &self,
        path: String,
        body: &R,
        context: &'static str,
    ) -> ApiResult<()> {
        let resp = self
            .inner
            .put(self.url(&path))
            .bearer_auth(&self.auth_bearer)
            .json(body)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("{context} transport: {e}")))?;
        Self::map_status_unit(resp, context).await
    }

    async fn delete_authed(&self, path: String, context: &'static str) -> ApiResult<()> {
        let resp = self
            .inner
            .delete(self.url(&path))
            .bearer_auth(&self.auth_bearer)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("{context} transport: {e}")))?;
        Self::map_status_unit(resp, context).await
    }

    fn enc(part: &str) -> String {
        // axum's matchit accepts most strings unescaped, but addresses
        // contain `@` and threads contain `:` etc. Percent-encode anything
        // not in the unreserved set.
        const RESERVED: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
            .add(b' ')
            .add(b'@')
            .add(b'/')
            .add(b':')
            .add(b'#')
            .add(b'?')
            .add(b'+')
            .add(b'%');
        percent_encoding::utf8_percent_encode(part, RESERVED).to_string()
    }

    // ── health ──────────────────────────────────────────────────────

    /// Healthz probe — NO auth (LB-reachable).
    pub async fn healthz(&self) -> ApiResult<types::HealthResponse> {
        let resp = self
            .inner
            .get(self.url(method::health::PATH_HEALTHZ))
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("healthz transport: {e}")))?;
        Self::map_status(resp, "healthz").await
    }

    /// Readyz — NO auth.
    pub async fn readyz(&self) -> ApiResult<types::HealthResponse> {
        let resp = self
            .inner
            .get(self.url(method::health::PATH_READYZ))
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("readyz transport: {e}")))?;
        Self::map_status(resp, "readyz").await
    }

    // ── conversation (Rock 1 + Rock 2) ──────────────────────────────

    /// POST /v1/users/{user}/conversations:list  (Rock 1)
    pub async fn list_conversations(
        &self,
        user: &str,
        req: &method::conversation::ListConversationsRequest,
    ) -> ApiResult<method::conversation::ListConversationsResponse> {
        let path = format!("/v1/users/{}/conversations:list", Self::enc(user));
        self.post_authed_json(path, req, "list_conversations").await
    }

    /// POST /v1/users/{user}/conversations:by-thread-ids
    pub async fn conversations_by_thread_ids(
        &self,
        user: &str,
        req: &method::conversation::ConversationsByIdsRequest,
    ) -> ApiResult<method::conversation::ConversationsByIdsResponse> {
        let path = format!("/v1/users/{}/conversations:by-thread-ids", Self::enc(user));
        self.post_authed_json(path, req, "conversations_by_thread_ids")
            .await
    }

    /// GET /v1/users/{user}/conversations/categories
    pub async fn conversation_categories(
        &self,
        user: &str,
    ) -> ApiResult<method::conversation::ConversationCategoriesResponse> {
        let path = format!("/v1/users/{}/conversations/categories", Self::enc(user));
        self.get_authed(path, "conversation_categories").await
    }

    /// GET /v1/users/{user}/conversations/action-count  (Rock 2)
    pub async fn action_count(
        &self,
        user: &str,
    ) -> ApiResult<method::conversation::ActionCountResponse> {
        let path = format!("/v1/users/{}/conversations/action-count", Self::enc(user));
        self.get_authed(path, "action_count").await
    }

    /// GET /v1/users/{user}/conversations/unseen-count  (Rock 2)
    pub async fn unseen_count(
        &self,
        user: &str,
    ) -> ApiResult<method::conversation::UnseenCountResponse> {
        let path = format!("/v1/users/{}/conversations/unseen-count", Self::enc(user));
        self.get_authed(path, "unseen_count").await
    }

    // ── thread read ─────────────────────────────────────────────────

    /// GET /v1/users/{user}/threads/{thread_id}/messages
    pub async fn list_thread_messages(
        &self,
        user: &str,
        thread_id: &str,
    ) -> ApiResult<method::thread::ListThreadMessagesResponse> {
        let path = format!(
            "/v1/users/{}/threads/{}/messages",
            Self::enc(user),
            Self::enc(thread_id)
        );
        self.get_authed(path, "list_thread_messages").await
    }

    // ── thread mutate ───────────────────────────────────────────────

    /// POST /v1/users/{user}/threads/{thread_id}/{action}
    /// — generic helper for mark_read/unread/star/unstar/pin/unpin/etc.
    async fn thread_action(
        &self,
        user: &str,
        thread_id: &str,
        action: &str,
        context: &'static str,
    ) -> ApiResult<method::thread::ThreadActionResponse> {
        let path = format!(
            "/v1/users/{}/threads/{}/{}",
            Self::enc(user),
            Self::enc(thread_id),
            action
        );
        self.post_authed_no_body(path, context).await
    }

    /// POST /v1/users/{user}/threads/{thread_id}/read
    pub async fn mark_thread_read(
        &self,
        user: &str,
        thread_id: &str,
    ) -> ApiResult<method::thread::ThreadActionResponse> {
        self.thread_action(user, thread_id, "read", "mark_thread_read")
            .await
    }

    /// POST /v1/users/{user}/threads/{thread_id}/unread
    pub async fn mark_thread_unread(
        &self,
        user: &str,
        thread_id: &str,
    ) -> ApiResult<method::thread::ThreadActionResponse> {
        self.thread_action(user, thread_id, "unread", "mark_thread_unread")
            .await
    }

    /// POST /v1/users/{user}/threads/{thread_id}/star
    pub async fn star_thread(
        &self,
        user: &str,
        thread_id: &str,
    ) -> ApiResult<method::thread::ThreadActionResponse> {
        self.thread_action(user, thread_id, "star", "star_thread")
            .await
    }

    /// POST /v1/users/{user}/threads/{thread_id}/unstar
    pub async fn unstar_thread(
        &self,
        user: &str,
        thread_id: &str,
    ) -> ApiResult<method::thread::ThreadActionResponse> {
        self.thread_action(user, thread_id, "unstar", "unstar_thread")
            .await
    }

    /// POST /v1/users/{user}/threads/{thread_id}/pin
    pub async fn pin_thread(
        &self,
        user: &str,
        thread_id: &str,
    ) -> ApiResult<method::thread::ThreadActionResponse> {
        self.thread_action(user, thread_id, "pin", "pin_thread")
            .await
    }

    /// POST /v1/users/{user}/threads/{thread_id}/unpin
    pub async fn unpin_thread(
        &self,
        user: &str,
        thread_id: &str,
    ) -> ApiResult<method::thread::ThreadActionResponse> {
        self.thread_action(user, thread_id, "unpin", "unpin_thread")
            .await
    }

    /// POST /v1/users/{user}/threads/{thread_id}/archive
    pub async fn archive_thread(
        &self,
        user: &str,
        thread_id: &str,
    ) -> ApiResult<method::thread::ThreadActionResponse> {
        self.thread_action(user, thread_id, "archive", "archive_thread")
            .await
    }

    /// POST /v1/users/{user}/threads/{thread_id}/unarchive
    pub async fn unarchive_thread(
        &self,
        user: &str,
        thread_id: &str,
    ) -> ApiResult<method::thread::ThreadActionResponse> {
        self.thread_action(user, thread_id, "unarchive", "unarchive_thread")
            .await
    }

    /// POST /v1/users/{user}/threads/{thread_id}/dismiss-action
    pub async fn dismiss_thread_action(
        &self,
        user: &str,
        thread_id: &str,
    ) -> ApiResult<method::thread::ThreadActionResponse> {
        self.thread_action(user, thread_id, "dismiss-action", "dismiss_action")
            .await
    }

    /// PUT /v1/users/{user}/threads/{thread_id}/snooze
    pub async fn snooze_thread(
        &self,
        user: &str,
        thread_id: &str,
        req: &method::thread::SnoozeRequest,
    ) -> ApiResult<()> {
        let path = format!(
            "/v1/users/{}/threads/{}/snooze",
            Self::enc(user),
            Self::enc(thread_id)
        );
        self.put_authed_json(path, req, "snooze_thread").await
    }

    /// DELETE /v1/users/{user}/threads/{thread_id}/snooze
    pub async fn unsnooze_thread(&self, user: &str, thread_id: &str) -> ApiResult<()> {
        let path = format!(
            "/v1/users/{}/threads/{}/snooze",
            Self::enc(user),
            Self::enc(thread_id)
        );
        self.delete_authed(path, "unsnooze_thread").await
    }

    /// DELETE /v1/users/{user}/threads/{thread_id}
    pub async fn delete_thread(&self, user: &str, thread_id: &str) -> ApiResult<()> {
        let path = format!(
            "/v1/users/{}/threads/{}",
            Self::enc(user),
            Self::enc(thread_id)
        );
        self.delete_authed(path, "delete_thread").await
    }

    // ── mailbox CRUD ────────────────────────────────────────────────

    /// GET /v1/users/{user}/mailboxes
    pub async fn list_mailboxes(
        &self,
        user: &str,
    ) -> ApiResult<method::mailbox::ListMailboxesResponse> {
        let path = format!("/v1/users/{}/mailboxes", Self::enc(user));
        self.get_authed(path, "list_mailboxes").await
    }

    /// GET /v1/mailboxes/{id}
    pub async fn get_mailbox_by_id(&self, id: i64) -> ApiResult<method::mailbox::MailboxWire> {
        let path = format!("/v1/mailboxes/{id}");
        self.get_authed(path, "get_mailbox_by_id").await
    }

    /// GET /v1/mailboxes/{id}/status
    pub async fn mailbox_status(
        &self,
        id: i64,
    ) -> ApiResult<method::mailbox::MailboxStatusResponse> {
        let path = format!("/v1/mailboxes/{id}/status");
        self.get_authed(path, "mailbox_status").await
    }

    // ── message read ────────────────────────────────────────────────

    /// GET /v1/mailboxes/{id}/messages/uid/{uid}
    pub async fn get_message_by_uid(
        &self,
        mailbox_id: i64,
        uid: u32,
    ) -> ApiResult<method::message::MessageWire> {
        let path = format!("/v1/mailboxes/{mailbox_id}/messages/uid/{uid}");
        self.get_authed(path, "get_message_by_uid").await
    }

    /// GET /v1/mailboxes/{id}/messages?offset=&limit=
    pub async fn list_messages(
        &self,
        mailbox_id: i64,
        offset: u32,
        limit: u32,
    ) -> ApiResult<method::message::ListMessagesResponse> {
        let path = format!("/v1/mailboxes/{mailbox_id}/messages?offset={offset}&limit={limit}");
        self.get_authed(path, "list_messages").await
    }

    // ── analysis ────────────────────────────────────────────────────

    /// GET /v1/analysis/{message_id}
    pub async fn get_analysis(
        &self,
        message_id: i64,
    ) -> ApiResult<method::analysis::GetAnalysisResponse> {
        let path = format!("/v1/analysis/{message_id}");
        self.get_authed(path, "get_analysis").await
    }

    // ── admin auth hot path ─────────────────────────────────────────

    /// GET /v1/admin/accounts/{address}/effective-permissions
    pub async fn effective_permissions(
        &self,
        address: &str,
    ) -> ApiResult<method::admin::EffectivePermissionsResponse> {
        let path = format!(
            "/v1/admin/accounts/{}/effective-permissions",
            Self::enc(address)
        );
        self.get_authed(path, "effective_permissions").await
    }

    /// GET /v1/admin/api-keys/by-prefix/{prefix}
    pub async fn api_key_by_prefix(&self, prefix: &str) -> ApiResult<method::admin::ApiKeyWire> {
        let path = format!("/v1/admin/api-keys/by-prefix/{}", Self::enc(prefix));
        self.get_authed(path, "api_key_by_prefix").await
    }

    /// POST /v1/admin/api-keys/{id}/touch
    pub async fn touch_api_key(&self, id: i64) -> ApiResult<()> {
        let path = format!("/v1/admin/api-keys/{id}/touch");
        let resp = self
            .inner
            .post(self.url(&path))
            .bearer_auth(&self.auth_bearer)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("touch_api_key transport: {e}")))?;
        Self::map_status_unit(resp, "touch_api_key").await
    }

    /// GET /v1/admin/accounts/{address}/credentials
    pub async fn get_account_with_hash(
        &self,
        address: &str,
    ) -> ApiResult<method::admin::AccountWithHashWire> {
        let path = format!("/v1/admin/accounts/{}/credentials", Self::enc(address));
        self.get_authed(path, "get_account_with_hash").await
    }

    // ── outbound queue (sender ↔ core) ──────────────────────────────

    /// POST /v1/outbound/claim — sender atomically claims up to N pending rows.
    pub async fn outbound_claim(
        &self,
        batch_size: u32,
    ) -> ApiResult<method::outbound::ClaimResponse> {
        let req = method::outbound::ClaimRequest { batch_size };
        self.post_authed_json(
            method::outbound::PATH_CLAIM.to_string(),
            &req,
            "outbound_claim",
        )
        .await
    }

    /// GET /v1/outbound/stats
    pub async fn outbound_stats(&self) -> ApiResult<method::outbound::QueueStatsResponse> {
        self.get_authed(method::outbound::PATH_STATS.to_string(), "outbound_stats")
            .await
    }

    /// POST /v1/outbound/recover-stale
    pub async fn outbound_recover_stale(
        &self,
        older_than_secs: u64,
    ) -> ApiResult<method::outbound::RecoverStaleResponse> {
        let req = method::outbound::RecoverStaleRequest { older_than_secs };
        self.post_authed_json(
            method::outbound::PATH_RECOVER_STALE.to_string(),
            &req,
            "outbound_recover_stale",
        )
        .await
    }
}
