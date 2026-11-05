//! What the ops daemon reads from a core. The client's bearer is the
//! daemon's own token here, not the core RPC secret.
//!
//! The route is mounted by `server::base_router` beside healthz and readyz,
//! not by either core's route table, so the path is named by its constant
//! as theirs are.

use crate::client::Client;
use crate::error::ApiResult;
use crate::method::ops::{OpsStatusWire, PATH_OPS_STATUS};

impl Client {
    /// GET /v1/ops/status
    pub async fn ops_status(&self) -> ApiResult<OpsStatusWire> {
        self.get_authed(PATH_OPS_STATUS.to_string(), "ops_status")
            .await
    }
}
