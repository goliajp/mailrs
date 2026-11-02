//! Team inboxes: assignment and reply claims, served by the core.

use crate::client::Client;
use crate::error::ApiResult;
use crate::method::shared as s;

impl Client {
    /// POST /v1/users/{user}/shared:assign
    pub async fn shared_assign(
        &self,
        user: &str,
        req: &s::AssignRequest,
    ) -> ApiResult<s::AssignResponse> {
        let path = format!("/v1/users/{}/shared:assign", Self::enc(user));
        self.post_authed_json(path, req, "shared_assign").await
    }

    /// POST /v1/users/{user}/shared:state
    pub async fn shared_state(
        &self,
        user: &str,
        req: &s::StateRequest,
    ) -> ApiResult<s::StateResponse> {
        let path = format!("/v1/users/{}/shared:state", Self::enc(user));
        self.post_authed_json(path, req, "shared_state").await
    }

    /// POST /v1/users/{user}/shared:claim-reply
    pub async fn shared_claim_reply(
        &self,
        user: &str,
        req: &s::ClaimReplyRequest,
    ) -> ApiResult<s::ClaimReplyResponse> {
        let path = format!("/v1/users/{}/shared:claim-reply", Self::enc(user));
        self.post_authed_json(path, req, "shared_claim_reply").await
    }

    /// POST /v1/users/{user}/shared:release-reply
    pub async fn shared_release_reply(
        &self,
        user: &str,
        req: &s::ReleaseReplyRequest,
    ) -> ApiResult<s::ReleaseReplyResponse> {
        let path = format!("/v1/users/{}/shared:release-reply", Self::enc(user));
        self.post_authed_json(path, req, "shared_release_reply")
            .await
    }
}
