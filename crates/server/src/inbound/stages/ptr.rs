//! PTR-check stage — FCrDNS scoring signal.

use std::sync::Arc;

use async_trait::async_trait;
use hickory_resolver::TokioResolver;
use mailrs_inbound::{ReceiveContext, Stage, StageOutcome};

/// Stage that runs `mailrs_shield::ptr::check_client_ptr` to score the
/// client's reverse-DNS posture. Always continues; writes `ctx.ptr_score`.
pub struct PtrStage {
    resolver: Arc<TokioResolver>,
}

impl PtrStage {
    /// Construct a `PtrStage` bound to a DNS resolver.
    pub fn new(resolver: Arc<TokioResolver>) -> Self {
        Self { resolver }
    }
}

#[async_trait]
impl Stage for PtrStage {
    fn name(&self) -> &str {
        "ptr"
    }

    async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
        ctx.ptr_score =
            mailrs_shield::ptr::check_client_ptr(&self.resolver, ctx.client_ip, &ctx.ehlo_domain)
                .await;
        StageOutcome::Continue
    }
}
