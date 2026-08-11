//! Conversation and thread RPCs.

use crate::client::Client;
use crate::error::ApiResult;
use crate::method;

impl Client {
    /// POST /v1/users/{user}/conversations:list  (Rock 1)
    pub async fn list_conversations(
        &self,
        user: &str,
        req: &method::conversation::ListConversationsRequest,
    ) -> ApiResult<method::conversation::ListConversationsResponse> {
        let path = format!("/v1/users/{}/conversations:list", Self::enc(user));
        self.post_authed_json(path, req, "list_conversations").await
    }

    /// POST /v1/users/{user}/conversations:search
    pub async fn search_conversations(
        &self,
        user: &str,
        req: &method::conversation::SearchConversationsRequest,
    ) -> ApiResult<method::conversation::SearchConversationsResponse> {
        let path = format!("/v1/users/{}/conversations:search", Self::enc(user));
        self.post_authed_json(path, req, "search_conversations")
            .await
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

    /// GET /v1/users/{user}/conversations/unseen-count  (Rock 2)
    pub async fn unseen_count(
        &self,
        user: &str,
    ) -> ApiResult<method::conversation::UnseenCountResponse> {
        let path = format!("/v1/users/{}/conversations/unseen-count", Self::enc(user));
        self.get_authed(path, "unseen_count").await
    }

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

    /// GET /v1/users/{user}/sent-messages
    pub async fn list_sent_messages(
        &self,
        user: &str,
    ) -> ApiResult<method::thread::SentMessagesResponse> {
        let path = format!("/v1/users/{}/sent-messages", Self::enc(user));
        self.get_authed(path, "list_sent_messages").await
    }

    /// GET /v1/users/{user}/threads/by-message-id/{message_id}
    pub async fn find_thread_by_message_id(
        &self,
        user: &str,
        message_id: &str,
    ) -> ApiResult<method::thread::FindThreadByMessageIdResponse> {
        let path = format!(
            "/v1/users/{}/threads/by-message-id/{}",
            Self::enc(user),
            Self::enc(message_id)
        );
        self.get_authed(path, "find_thread_by_message_id").await
    }

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

    /// POST /v1/users/{user}/conversations:mark-all-read — flip every
    /// unread thread server-side. Returns the count that was flipped.
    pub async fn mark_all_conversations_read(&self, user: &str) -> ApiResult<u32> {
        let path = format!("/v1/users/{}/conversations:mark-all-read", Self::enc(user));
        let v: serde_json::Value = self
            .post_authed_no_body(path, "mark_all_conversations_read")
            .await?;
        Ok(v.get("flipped").and_then(|f| f.as_u64()).unwrap_or(0) as u32)
    }

    /// POST /v1/users/{user}/conversations:mark-list-read — flip every
    /// unread thread **this list is showing**, using the same filter
    /// the list read takes. Returns the count that was flipped.
    ///
    /// The mailbox-wide version above is the wrong answer when someone
    /// is looking at one folder: marking all read from inside
    /// Notifications should not silence the inbox.
    pub async fn mark_list_conversations_read(
        &self,
        user: &str,
        filter: &crate::types::ConversationFilter,
    ) -> ApiResult<u32> {
        let path = format!("/v1/users/{}/conversations:mark-list-read", Self::enc(user));
        let body = crate::method::conversation::ListConversationsRequest {
            filter: filter.clone(),
        };
        let v: serde_json::Value = self
            .post_authed_json(path, &body, "mark_list_conversations_read")
            .await?;
        Ok(v.get("flipped").and_then(|f| f.as_u64()).unwrap_or(0) as u32)
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

    /// POST /v1/users/{user}/threads/{thread_id}/mark-junk
    /// v2.4.1 Phase 3 (RFC-B §3.4).
    pub async fn mark_junk(
        &self,
        user: &str,
        thread_id: &str,
    ) -> ApiResult<method::thread::ThreadActionResponse> {
        self.thread_action(user, thread_id, "mark-junk", "mark_junk")
            .await
    }

    /// POST /v1/users/{user}/threads/{thread_id}/mark-not-junk
    /// v2.4.1 Phase 3 (RFC-B §3.4).
    pub async fn mark_not_junk(
        &self,
        user: &str,
        thread_id: &str,
    ) -> ApiResult<method::thread::ThreadActionResponse> {
        self.thread_action(user, thread_id, "mark-not-junk", "mark_not_junk")
            .await
    }

    /// POST /v1/users/{user}/threads/{thread_id}/mark-notification
    /// v2.9 triage.
    pub async fn mark_notification(
        &self,
        user: &str,
        thread_id: &str,
    ) -> ApiResult<method::thread::ThreadActionResponse> {
        self.thread_action(user, thread_id, "mark-notification", "mark_notification")
            .await
    }

    /// POST /v1/users/{user}/threads/{thread_id}/mark-promotion
    /// v2.9 triage.
    pub async fn mark_promotion(
        &self,
        user: &str,
        thread_id: &str,
    ) -> ApiResult<method::thread::ThreadActionResponse> {
        self.thread_action(user, thread_id, "mark-promotion", "mark_promotion")
            .await
    }

    /// POST /v1/users/{user}/threads/{thread_id}/move-to-inbox
    /// v2.9 triage.
    pub async fn move_to_inbox(
        &self,
        user: &str,
        thread_id: &str,
    ) -> ApiResult<method::thread::ThreadActionResponse> {
        self.thread_action(user, thread_id, "move-to-inbox", "move_to_inbox")
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

    /// DELETE /v1/users/{user}/threads/{thread_id}/unsnooze
    pub async fn unsnooze_thread(&self, user: &str, thread_id: &str) -> ApiResult<()> {
        let path = format!(
            "/v1/users/{}/threads/{}/unsnooze",
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
}
