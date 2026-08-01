//! The outbound queue RPCs the sender drives.

use crate::client::Client;
use crate::error::{ApiResult, CoreApiError};
use crate::method;

impl Client {
    /// POST /v1/outbound/enqueue — webapi /api/mail/send write path.
    pub async fn outbound_enqueue(
        &self,
        req: &method::outbound::EnqueueRequest,
    ) -> ApiResult<method::outbound::EnqueueResponse> {
        self.post_authed_json(
            method::outbound::PATH_ENQUEUE.to_string(),
            req,
            "outbound_enqueue",
        )
        .await
    }

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

    /// POST /v1/outbound/{id}/delivered
    pub async fn outbound_mark_delivered(&self, id: i64) -> ApiResult<()> {
        let path = format!("/v1/outbound/{id}/delivered");
        let resp = self
            .inner
            .post(self.url(&path))
            .bearer_auth(&self.auth_bearer)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("mark_delivered transport: {e}")))?;
        Self::map_status_unit(resp, "outbound_mark_delivered").await
    }

    /// POST /v1/outbound/{id}/failed
    pub async fn outbound_mark_failed(&self, id: i64, error: String) -> ApiResult<()> {
        let path = format!("/v1/outbound/{id}/failed");
        let req = method::outbound::MarkFailedRequest {
            error,
            next_retry: None,
        };
        let resp = self
            .inner
            .post(self.url(&path))
            .bearer_auth(&self.auth_bearer)
            .json(&req)
            .send()
            .await
            .map_err(|e| CoreApiError::Internal(format!("mark_failed transport: {e}")))?;
        Self::map_status_unit(resp, "outbound_mark_failed").await
    }
}
