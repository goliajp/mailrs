//! Read-only operator checks both cores answer from the network kevy and
//! their store. Their reports are the shared crates' types; the client
//! hands them back as JSON.

use crate::client::Client;
use crate::error::ApiResult;

impl Client {
    /// POST /v1/admin/maintenance:audit-verify — the audit chain checked
    /// now, and against its replica when one is configured
    pub async fn audit_verify(&self) -> ApiResult<serde_json::Value> {
        self.post_authed_no_body(
            "/v1/admin/maintenance:audit-verify".to_string(),
            "audit_verify",
        )
        .await
    }

    /// GET /v1/admin/maintenance:sieve-survey — which stored Sieve scripts
    /// redirect out of our domains, and how many vacation replies name
    /// another sender
    pub async fn sieve_survey(&self) -> ApiResult<serde_json::Value> {
        self.get_authed(
            "/v1/admin/maintenance:sieve-survey".to_string(),
            "sieve_survey",
        )
        .await
    }
}
