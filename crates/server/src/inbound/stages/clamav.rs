//! ClamAV virus-scan stage.

use async_trait::async_trait;
use mailrs_inbound::{ReceiveContext, Stage, StageOutcome};

use crate::inbound::content_scan::{scan_clamav, ClamavResult};

/// Stage that streams the message body to a ClamAV `clamd` socket and
/// writes `ctx.virus_found = Some(name)` on detection. Always continues —
/// the final decision step inspects `virus_found` to issue a 550 reject.
///
/// On scan error (connect/read failure) the stage logs and continues with
/// no signal mutation, treating ClamAV as a best-effort check.
pub struct ClamavStage {
    addr: String,
}

impl ClamavStage {
    /// Construct a `ClamavStage` pointing at a `clamd` TCP endpoint
    /// (e.g. `"127.0.0.1:3310"`).
    pub fn new(addr: String) -> Self {
        Self { addr }
    }
}

#[async_trait]
impl Stage for ClamavStage {
    fn name(&self) -> &str {
        "clamav"
    }

    async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
        match scan_clamav(&self.addr, &ctx.message).await {
            ClamavResult::Virus(name) => {
                tracing::warn!(event = "clamav_reject", virus = %name, "virus detected");
                ctx.virus_found = Some(name);
            }
            ClamavResult::Error(e) => {
                tracing::warn!(event = "clamav_error", error = %e, "ClamAV scan failed, accepting");
            }
            ClamavResult::Clean => {}
        }
        StageOutcome::Continue
    }
}
