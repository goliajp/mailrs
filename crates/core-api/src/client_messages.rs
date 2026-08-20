//! Message, mailbox and analysis RPCs.

use crate::client::Client;
use crate::error::{ApiResult, CoreApiError};
use crate::method;

impl Client {
    /// POST /v1/users/{user}/threads/{thread_id}/messages — deliver a
    /// synthesized message (sent copy, draft, import) into the user's
    /// kevy view. Used by the webapi send / save-draft handlers to
    /// mirror the outbound message so it shows up in Sent / Drafts.
    pub async fn deliver_message(
        &self,
        user: &str,
        thread_id: &str,
        req: &method::thread::DeliverMessageRequest,
    ) -> ApiResult<method::thread::DeliverMessageResponse> {
        let path = format!(
            "/v1/users/{}/threads/{}/messages",
            Self::enc(user),
            Self::enc(thread_id)
        );
        self.post_authed_json(path, req, "deliver_message").await
    }

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

    /// GET /v1/mailboxes/{id}/messages/uid/{uid}
    pub async fn get_message_by_uid(
        &self,
        mailbox_id: i64,
        uid: u32,
    ) -> ApiResult<method::message::MessageWire> {
        let path = format!("/v1/mailboxes/{mailbox_id}/messages/uid/{uid}");
        self.get_authed(path, "get_message_by_uid").await
    }

    /// GET /v1/users/{user}/messages/by-uid/{uid} — fastcore-native
    /// variant. Resolves through the per-user uid index instead of a
    /// per-mailbox scan. Preferred when the caller already knows the
    /// user (webapi does).
    pub async fn get_message_by_uid_for_user(
        &self,
        user: &str,
        uid: u32,
    ) -> ApiResult<method::message::MessageWire> {
        let path = format!("/v1/users/{}/messages/by-uid/{uid}", Self::enc(user));
        self.get_authed(path, "get_message_by_uid_for_user").await
    }

    /// GET /v1/users/{user}/messages/by-uid/{uid}/invite — the typed
    /// invitation this message carries, as JSON, or 404 when it carries
    /// none.
    ///
    /// Returned untyped because the shape is `mailrs_ical::ParsedInvite`
    /// and the caller hands it straight to the browser; re-declaring it
    /// here would buy nothing and add a second place for it to drift.
    pub async fn get_invite(&self, user: &str, uid: u32) -> ApiResult<serde_json::Value> {
        let path = format!("/v1/users/{}/messages/by-uid/{uid}/invite", Self::enc(user));
        self.get_authed(path, "get_invite").await
    }

    /// GET /v1/users/{user}/messages/by-message-id/{message_id} —
    /// resolve a MessageWire from its RFC 5322 Message-ID. Used by
    /// webapi's `/api/mail/send` when the compose request carries
    /// `forward_message_id` and needs to read the original .eml to
    /// inline its body into the forward.
    pub async fn find_by_message_id_for_user(
        &self,
        user: &str,
        message_id: &str,
    ) -> ApiResult<method::message::MessageWire> {
        let path = format!(
            "/v1/users/{}/messages/by-message-id/{}",
            Self::enc(user),
            Self::enc(message_id),
        );
        self.get_authed(path, "find_by_message_id_for_user").await
    }

    /// GET /v1/mailboxes/{id}/messages/uid/{uid}/raw  → raw RFC 5322 bytes.
    pub async fn get_message_raw(&self, mailbox_id: i64, uid: u32) -> ApiResult<Vec<u8>> {
        let path = format!("/v1/mailboxes/{mailbox_id}/messages/uid/{uid}/raw");
        let resp = self
            .inner
            .get(self.url(&path))
            .bearer_auth(&self.auth_bearer)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("get_message_raw transport: {e}")))?;
        let status = resp.status().as_u16();
        match status {
            200..=299 => resp
                .bytes()
                .await
                .map(|b| b.to_vec())
                .map_err(|e| CoreApiError::Internal(format!("get_message_raw read: {e}"))),
            401 => Err(CoreApiError::Unauthorized),
            404 => Err(CoreApiError::NotFound("get_message_raw".into())),
            other => Err(CoreApiError::Internal(format!(
                "get_message_raw returned {other}"
            ))),
        }
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

    /// GET /v1/analysis/{message_id}
    pub async fn get_analysis(
        &self,
        message_id: i64,
    ) -> ApiResult<method::analysis::GetAnalysisResponse> {
        let path = format!("/v1/analysis/{message_id}");
        self.get_authed(path, "get_analysis").await
    }

    /// POST /v1/users/{user}/messages/{uid}/flags — patch a message's
    /// flag bitmask. Fastcore reconciles the thread's has_unread zset
    /// when `\Seen` toggles.
    pub async fn set_message_flags(
        &self,
        user: &str,
        uid: u32,
        req: &method::admin::SetMessageFlagsRequest,
    ) -> ApiResult<()> {
        let path = format!("/v1/users/{}/messages/{}/flags", Self::enc(user), uid);
        self.post_authed_json(path, req, "set_message_flags").await
    }
}
