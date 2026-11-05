//! What the ops daemon reads from a core. The client's bearer is the
//! daemon's own token here, not the core RPC secret.
//!
//! The route is mounted by `server::base_router` beside healthz and readyz,
//! not by either core's route table, so the path is named by its constant
//! as theirs are.

use crate::client::{Client, transport};
use crate::error::ApiResult;
use crate::method::ops::{FIRST_READ_HEADER, OpsStatusWire, PATH_OPS_STATUS};

impl Client {
    /// GET /v1/ops/status. `first_read` marks the daemon's first read
    /// since it started, which the core writes to the audit log.
    pub async fn ops_status(&self, first_read: bool) -> ApiResult<OpsStatusWire> {
        let mut req = self
            .inner
            .get(self.url(PATH_OPS_STATUS))
            .bearer_auth(&self.auth_bearer);
        if first_read {
            req = req.header(FIRST_READ_HEADER, "1");
        }
        let resp = req.send().await.map_err(|e| transport("ops_status", e))?;
        Self::map_status(resp, "ops_status").await
    }
}
