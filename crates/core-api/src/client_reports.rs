//! Reporting a scam, and an administrator deciding on the other copies.

use crate::client::Client;
use crate::error::ApiResult;
use crate::method::report as r;

impl Client {
    /// POST /v1/users/{user}/messages/{message_id}/report
    pub async fn report_message(
        &self,
        user: &str,
        message_id: &str,
        administrator: bool,
    ) -> ApiResult<r::ReportOutcomeWire> {
        let path = format!(
            "/v1/users/{}/messages/{}/report",
            Self::enc(user),
            Self::enc(message_id)
        );
        let req = r::ReportRequest { administrator };
        self.post_authed_json(path, &req, "report_message").await
    }

    /// GET /v1/users/{user}/reports/{report_id}
    pub async fn report_outcome(
        &self,
        user: &str,
        report_id: &str,
    ) -> ApiResult<r::ReportOutcomeWire> {
        let path = format!(
            "/v1/users/{}/reports/{}",
            Self::enc(user),
            Self::enc(report_id)
        );
        self.get_authed(path, "report_outcome").await
    }

    /// GET /v1/admin/reports/pending
    pub async fn pending_reports(&self) -> ApiResult<r::PendingReportsResponse> {
        self.get_authed("/v1/admin/reports/pending".to_string(), "pending_reports")
            .await
    }

    /// POST /v1/admin/reports/{report_id}/confirm
    pub async fn confirm_report(
        &self,
        report_id: &str,
        actor: &str,
    ) -> ApiResult<r::ReportOutcomeWire> {
        let path = format!("/v1/admin/reports/{}/confirm", Self::enc(report_id));
        let req = r::DecideReportRequest {
            actor: actor.to_string(),
        };
        self.post_authed_json(path, &req, "confirm_report").await
    }

    /// POST /v1/admin/reports/{report_id}/dismiss
    pub async fn dismiss_report(
        &self,
        report_id: &str,
        actor: &str,
    ) -> ApiResult<r::ReportOutcomeWire> {
        let path = format!("/v1/admin/reports/{}/dismiss", Self::enc(report_id));
        let req = r::DecideReportRequest {
            actor: actor.to_string(),
        };
        self.post_authed_json(path, &req, "dismiss_report").await
    }
}
