//! What the ops daemon reads from a core. The client's bearer is the
//! daemon's own token here, not the core RPC secret.

use crate::client::Client;
use crate::error::ApiResult;
use crate::method::ops::OpsStatusWire;

impl Client {
    /// GET /v1/ops/status
    pub async fn ops_status(&self) -> ApiResult<OpsStatusWire> {
        self.get_authed("/v1/ops/status".to_string(), "ops_status")
            .await
    }
}
