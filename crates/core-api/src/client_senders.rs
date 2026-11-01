//! A reader's rules for one sender, applied by the core.

use crate::client::Client;
use crate::error::ApiResult;
use crate::method::senders as s;

impl Client {
    /// POST /v1/users/{user}/senders:reclassify
    pub async fn reclassify_sender(
        &self,
        user: &str,
        req: &s::ReclassifySenderRequest,
    ) -> ApiResult<s::ReclassifySenderResponse> {
        let path = format!("/v1/users/{}/senders:reclassify", Self::enc(user));
        self.post_authed_json(path, req, "reclassify_sender").await
    }

    /// POST /v1/users/{user}/senders:restore
    pub async fn restore_sender(
        &self,
        user: &str,
        req: &s::RestoreSenderRequest,
    ) -> ApiResult<s::RestoreSenderResponse> {
        let path = format!("/v1/users/{}/senders:restore", Self::enc(user));
        self.post_authed_json(path, req, "restore_sender").await
    }

    /// POST /v1/users/{user}/senders:retain
    pub async fn retain_sender(
        &self,
        user: &str,
        req: &s::RetainSenderRequest,
    ) -> ApiResult<s::RetainSenderResponse> {
        let path = format!("/v1/users/{}/senders:retain", Self::enc(user));
        self.post_authed_json(path, req, "retain_sender").await
    }
}
