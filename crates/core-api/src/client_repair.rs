//! Operator repairs of a store's conversations.

use crate::client::Client;
use crate::error::ApiResult;
use crate::method::rethread::{SplitMessageRequest, SplitMessageResponse};

impl Client {
    /// POST /v1/admin/threads:split-message — move `message_id`, one of
    /// `user`'s messages, into a conversation of its own. 404 when `user`
    /// has no such message.
    pub async fn split_message(
        &self,
        user: &str,
        message_id: &str,
    ) -> ApiResult<SplitMessageResponse> {
        let req = SplitMessageRequest {
            user: user.to_string(),
            message_id: message_id.to_string(),
        };
        // spelled out, as the contract checks read the paths the client asks for
        self.post_authed_json(
            "/v1/admin/threads:split-message".to_string(),
            &req,
            "split_message",
        )
        .await
    }
}
