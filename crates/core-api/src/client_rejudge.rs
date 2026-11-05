//! Re-judge batch RPCs: what re-judging took back, and the decisions on it.

use crate::client::Client;
use crate::error::ApiResult;
use crate::method;

impl Client {
    /// GET /v1/admin/rejudge-batches — what re-judging took back, by round
    pub async fn rejudge_batches(&self) -> ApiResult<method::admin::RejudgeBatchesResponse> {
        self.get_authed("/v1/admin/rejudge-batches".to_string(), "rejudge_batches")
            .await
    }

    /// POST /v1/admin/rejudge-batches/{observation}/release — put a round back
    pub async fn release_rejudge_batch(
        &self,
        observation: &str,
        actor: &str,
    ) -> ApiResult<method::admin::ReleaseRejudgeBatchResponse> {
        let path = format!(
            "/v1/admin/rejudge-batches/{}/release",
            Self::enc(observation)
        );
        let req = method::admin::ReleaseRejudgeBatchRequest {
            actor: actor.to_string(),
        };
        self.post_authed_json(path, &req, "release_rejudge_batch")
            .await
    }

    /// POST /v1/admin/rejudge-batches/{observation}/confirm — hold what a
    /// pending round recorded
    pub async fn confirm_rejudge_batch(
        &self,
        observation: &str,
        actor: &str,
    ) -> ApiResult<method::admin::ConfirmRejudgeBatchResponse> {
        let path = format!(
            "/v1/admin/rejudge-batches/{}/confirm",
            Self::enc(observation)
        );
        let req = method::admin::DecideRejudgeBatchRequest {
            actor: actor.to_string(),
        };
        self.post_authed_json(path, &req, "confirm_rejudge_batch")
            .await
    }

    /// POST /v1/admin/rejudge-batches/{observation}/discard — hold nothing
    /// of a pending round
    pub async fn discard_rejudge_batch(
        &self,
        observation: &str,
        actor: &str,
    ) -> ApiResult<method::admin::DiscardRejudgeBatchResponse> {
        let path = format!(
            "/v1/admin/rejudge-batches/{}/discard",
            Self::enc(observation)
        );
        let req = method::admin::DecideRejudgeBatchRequest {
            actor: actor.to_string(),
        };
        self.post_authed_json(path, &req, "discard_rejudge_batch")
            .await
    }
}
