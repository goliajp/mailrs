//! The [`Stage`] trait — one check that runs as part of a [`crate::Pipeline`].
//!
//! A stage reads + mutates the [`ReceiveContext`] and
//! returns either [`StageOutcome::Continue`] (move on to the next stage) or
//! [`StageOutcome::Decide`] (terminal — pipeline returns this decision).
//!
//! The trait is intentionally one-method-shape. Stages are free to internally
//! call DNS resolvers, virus scanners, store backends, LLM providers, etc;
//! the trait says nothing about those.

use async_trait::async_trait;

use crate::context::ReceiveContext;
use crate::decision::DeliveryDecision;

/// Outcome of one [`Stage::evaluate`] call.
///
/// `Continue` keeps the pipeline running; `Decide` short-circuits.
#[derive(Debug, Clone, PartialEq)]
pub enum StageOutcome {
    /// No terminal decision yet — move to the next stage with the
    /// (possibly mutated) context.
    Continue,
    /// Terminal — stop the pipeline immediately and return this decision.
    Decide(DeliveryDecision),
}

/// One check in a receive pipeline.
///
/// Implementations typically wrap a single backend (a DNS resolver, a virus
/// scanner socket, a key-value store) and translate its result into a
/// signal mutation on the context (writing `ctx.virus_found`,
/// `ctx.ptr_score`, etc) and/or a decision (returning
/// `StageOutcome::Decide(...)`).
///
/// ## Contract notes
///
/// - Stages run **sequentially** in the order they were added to the
///   pipeline. A stage that returns `Continue` cannot prevent the next
///   stage from running.
/// - **Errors are the stage's responsibility.** The trait doesn't carry
///   `Result`. If your stage's internal backend fails, it can:
///   - Log and return `Continue` (treat the check as "no signal").
///   - Return `Decide(Reject { code: 451, message: ... })` for a soft
///     temporary failure.
///   - Return `Decide(Reject { code: 550, message: ... })` for a hard
///     failure if your policy says "no DKIM, no delivery".
/// - **The context is mutable by design.** Most stages write 1-2 fields
///   (e.g. a PTR-check stage writes `ctx.ptr_score`). Subsequent stages
///   can read those values.
/// - Stages must be `Send + Sync` because the pipeline is async; per-stage
///   mutable state requires interior mutability (e.g. `Mutex` / `DashMap`).
#[async_trait]
pub trait Stage: Send + Sync {
    /// Human-readable name for tracing / metrics. Should be short,
    /// snake_case, stable across versions (e.g. `"greylist"`, `"dkim"`,
    /// `"clamav"`).
    fn name(&self) -> &str;

    /// Evaluate this stage. Reads + mutates `ctx`, then returns whether
    /// to continue or short-circuit.
    async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome;
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    /// A no-op stage for trait-shape testing.
    struct NoopStage;

    #[async_trait]
    impl Stage for NoopStage {
        fn name(&self) -> &str {
            "noop"
        }
        async fn evaluate(&self, _ctx: &mut ReceiveContext) -> StageOutcome {
            StageOutcome::Continue
        }
    }

    /// A stage that always rejects.
    struct AlwaysRejectStage;

    #[async_trait]
    impl Stage for AlwaysRejectStage {
        fn name(&self) -> &str {
            "always_reject"
        }
        async fn evaluate(&self, _ctx: &mut ReceiveContext) -> StageOutcome {
            StageOutcome::Decide(DeliveryDecision::Reject {
                code: 550,
                message: "5.7.1 nope".into(),
            })
        }
    }

    fn ctx() -> ReceiveContext {
        ReceiveContext::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            "ehlo",
            "a@x",
            "b@x",
            b"body".to_vec(),
            "mx",
        )
    }

    #[tokio::test]
    async fn noop_stage_returns_continue() {
        let s = NoopStage;
        let mut c = ctx();
        assert_eq!(s.evaluate(&mut c).await, StageOutcome::Continue);
    }

    #[tokio::test]
    async fn always_reject_returns_decide() {
        let s = AlwaysRejectStage;
        let mut c = ctx();
        match s.evaluate(&mut c).await {
            StageOutcome::Decide(DeliveryDecision::Reject { code, .. }) => {
                assert_eq!(code, 550);
            }
            other => panic!("expected reject, got {other:?}"),
        }
    }

    #[test]
    fn stage_name_is_callable_on_trait_object() {
        let s: Box<dyn Stage> = Box::new(NoopStage);
        assert_eq!(s.name(), "noop");
    }
}
