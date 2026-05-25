//! The [`Pipeline`] executor + its builder.
//!
//! A `Pipeline` is a sequence of [`Stage`]s plus a spam-score threshold.
//! [`Pipeline::run`] evaluates the stages in order on a [`ReceiveContext`],
//! short-circuiting on the first `Decide` outcome. If every stage returns
//! `Continue`, the final decision is computed by
//! [`make_delivery_decision`] over the accumulated signals.

use crate::context::ReceiveContext;
use crate::decision::{make_delivery_decision, DeliveryDecision};
use crate::stage::{Stage, StageOutcome};

/// Default spam-score threshold above which the message goes to Junk.
///
/// Picked to be sensitive enough to catch obvious bulk mail (content score
/// 3-4 + tracking pixel + non-FCrDNS sender) but not so aggressive that
/// legitimate mail with one minor signal gets junked.
pub const DEFAULT_SPAM_THRESHOLD: f64 = 5.0;

/// Composable receive pipeline.
///
/// Build via [`Pipeline::builder`], then call [`Pipeline::run`] for each
/// inbound message. A single `Pipeline` is shareable across requests
/// (typically `Arc<Pipeline>`).
pub struct Pipeline {
    stages: Vec<Box<dyn Stage>>,
    spam_threshold: f64,
}

impl Pipeline {
    /// Begin building a pipeline.
    pub fn builder() -> PipelineBuilder {
        PipelineBuilder {
            stages: Vec::new(),
            spam_threshold: DEFAULT_SPAM_THRESHOLD,
        }
    }

    /// Run the pipeline on `ctx`. Stages are evaluated in the order they
    /// were added; the first `Decide(_)` short-circuits.
    ///
    /// If every stage returns `Continue`, the final decision is computed
    /// from the accumulated signals via
    /// [`make_delivery_decision`].
    ///
    /// **Tracing.** Emits one `info_span!("inbound.pipeline", n_stages, …)`
    /// for the whole run + one nested `debug_span!("inbound.stage",
    /// name=…)` per evaluated stage. Each per-stage future is attached
    /// via `tracing::Instrument` so the span correctly survives `.await`
    /// suspension. If a `tracing-subscriber` is set up and the caller's
    /// connection handler is itself in a span (e.g. `smtp.conn`), the
    /// pipeline span nests under it automatically.
    #[tracing::instrument(
        name = "inbound.pipeline",
        skip(self, ctx),
        fields(n_stages = self.stages.len(), spam_threshold = self.spam_threshold),
    )]
    pub async fn run(&self, ctx: &mut ReceiveContext) -> DeliveryDecision {
        use tracing::Instrument;
        for stage in &self.stages {
            let stage_span = tracing::debug_span!("inbound.stage", name = stage.name());
            let outcome = stage.evaluate(ctx).instrument(stage_span).await;
            if let StageOutcome::Decide(d) = outcome {
                tracing::debug!(stage = stage.name(), "pipeline short-circuit");
                return d;
            }
        }
        let decision = make_delivery_decision(&ctx.to_pipeline_input(self.spam_threshold));
        tracing::debug!(?decision, "pipeline ran to completion");
        decision
    }

    /// Iterate over stage names, in order. Useful for introspection /
    /// debug logging.
    pub fn stage_names(&self) -> impl Iterator<Item = &str> {
        self.stages.iter().map(|s| s.name())
    }

    /// The configured spam-score threshold.
    pub fn spam_threshold(&self) -> f64 {
        self.spam_threshold
    }
}

/// Builder for [`Pipeline`]. Chain `.add(...)` calls then `.build()`.
pub struct PipelineBuilder {
    stages: Vec<Box<dyn Stage>>,
    spam_threshold: f64,
}

impl PipelineBuilder {
    /// Append a stage. Stages run in the order they were added.
    #[allow(clippy::should_implement_trait)]
    pub fn add<S: Stage + 'static>(mut self, stage: S) -> Self {
        self.stages.push(Box::new(stage));
        self
    }

    /// Override the spam-score threshold (default: [`DEFAULT_SPAM_THRESHOLD`]).
    pub fn spam_threshold(mut self, threshold: f64) -> Self {
        self.spam_threshold = threshold;
        self
    }

    /// Finish the builder and return the [`Pipeline`].
    pub fn build(self) -> Pipeline {
        Pipeline {
            stages: self.stages,
            spam_threshold: self.spam_threshold,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::*;
    use crate::context::DmarcPolicy;

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

    struct RecordingStage {
        name: &'static str,
        counter: Arc<AtomicUsize>,
        outcome: StageOutcome,
    }

    #[async_trait]
    impl Stage for RecordingStage {
        fn name(&self) -> &str {
            self.name
        }
        async fn evaluate(&self, _ctx: &mut ReceiveContext) -> StageOutcome {
            self.counter.fetch_add(1, Ordering::SeqCst);
            self.outcome.clone()
        }
    }

    struct ScoringStage(f64);

    #[async_trait]
    impl Stage for ScoringStage {
        fn name(&self) -> &str {
            "scoring"
        }
        async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
            ctx.content_score += self.0;
            StageOutcome::Continue
        }
    }

    #[tokio::test]
    async fn empty_pipeline_falls_through_to_accept() {
        let p = Pipeline::builder().build();
        let mut c = ctx();
        c.auth_results.dmarc_policy = DmarcPolicy::Pass;
        match p.run(&mut c).await {
            DeliveryDecision::Accept { .. } => {}
            other => panic!("expected Accept, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stages_run_in_order_when_all_continue() {
        let counter = Arc::new(AtomicUsize::new(0));
        let p = Pipeline::builder()
            .add(RecordingStage {
                name: "first",
                counter: counter.clone(),
                outcome: StageOutcome::Continue,
            })
            .add(RecordingStage {
                name: "second",
                counter: counter.clone(),
                outcome: StageOutcome::Continue,
            })
            .add(RecordingStage {
                name: "third",
                counter: counter.clone(),
                outcome: StageOutcome::Continue,
            })
            .build();
        let mut c = ctx();
        p.run(&mut c).await;
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn early_decide_short_circuits_remaining_stages() {
        let counter = Arc::new(AtomicUsize::new(0));
        let p = Pipeline::builder()
            .add(RecordingStage {
                name: "first",
                counter: counter.clone(),
                outcome: StageOutcome::Continue,
            })
            .add(RecordingStage {
                name: "second",
                counter: counter.clone(),
                outcome: StageOutcome::Decide(DeliveryDecision::Greylist),
            })
            .add(RecordingStage {
                name: "third_should_not_run",
                counter: counter.clone(),
                outcome: StageOutcome::Continue,
            })
            .build();
        let mut c = ctx();
        assert_eq!(p.run(&mut c).await, DeliveryDecision::Greylist);
        assert_eq!(
            counter.load(Ordering::SeqCst),
            2,
            "stage 3 must not have run"
        );
    }

    #[tokio::test]
    async fn stages_can_mutate_context_signals() {
        let p = Pipeline::builder()
            .add(ScoringStage(3.0))
            .add(ScoringStage(2.5))
            .spam_threshold(5.0)
            .build();
        let mut c = ctx();
        c.auth_results.dmarc_policy = DmarcPolicy::Pass;
        match p.run(&mut c).await {
            DeliveryDecision::Junk { reason, .. } => {
                // 3.0 + 2.5 = 5.5 ≥ 5.0
                assert!(reason.contains("5.5"));
            }
            other => panic!("expected Junk, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn spam_threshold_default_is_five() {
        let p = Pipeline::builder().build();
        assert_eq!(p.spam_threshold(), DEFAULT_SPAM_THRESHOLD);
        assert_eq!(p.spam_threshold(), 5.0);
    }

    #[tokio::test]
    async fn custom_spam_threshold_is_threaded_through() {
        let p = Pipeline::builder()
            .add(ScoringStage(4.5))
            .spam_threshold(10.0) // raise the bar
            .build();
        let mut c = ctx();
        c.auth_results.dmarc_policy = DmarcPolicy::Pass;
        // 4.5 < 10.0, should accept
        assert!(matches!(
            p.run(&mut c).await,
            DeliveryDecision::Accept { .. }
        ));
    }

    #[tokio::test]
    async fn stage_names_introspection() {
        let counter = Arc::new(AtomicUsize::new(0));
        let p = Pipeline::builder()
            .add(RecordingStage {
                name: "alpha",
                counter: counter.clone(),
                outcome: StageOutcome::Continue,
            })
            .add(RecordingStage {
                name: "beta",
                counter,
                outcome: StageOutcome::Continue,
            })
            .build();
        let names: Vec<&str> = p.stage_names().collect();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    // ===== Additional corner-case tests =====

    #[tokio::test]
    async fn mixed_continue_and_decide_short_circuits_only_at_decide() {
        // Pattern: continue, continue, decide(reject), continue
        // Only first three stages should run.
        let counter = Arc::new(AtomicUsize::new(0));
        let p = Pipeline::builder()
            .add(RecordingStage {
                name: "a",
                counter: counter.clone(),
                outcome: StageOutcome::Continue,
            })
            .add(RecordingStage {
                name: "b",
                counter: counter.clone(),
                outcome: StageOutcome::Continue,
            })
            .add(RecordingStage {
                name: "c-decides",
                counter: counter.clone(),
                outcome: StageOutcome::Decide(DeliveryDecision::Reject {
                    code: 550,
                    message: "5.7.1 blocked".into(),
                }),
            })
            .add(RecordingStage {
                name: "d-never-runs",
                counter: counter.clone(),
                outcome: StageOutcome::Continue,
            })
            .build();
        let mut c = ctx();
        let decision = p.run(&mut c).await;
        match decision {
            DeliveryDecision::Reject { code, .. } => assert_eq!(code, 550),
            other => panic!("expected Reject, got {other:?}"),
        }
        assert_eq!(counter.load(Ordering::SeqCst), 3, "only 3 stages ran (a,b,c-decides)");
    }

    #[tokio::test]
    async fn stage_names_match_add_order_exactly() {
        // Verify stage_names preserves insertion order (Vec semantics).
        let counter = Arc::new(AtomicUsize::new(0));
        let p = Pipeline::builder()
            .add(RecordingStage { name: "rate-limit", counter: counter.clone(), outcome: StageOutcome::Continue })
            .add(RecordingStage { name: "ptr", counter: counter.clone(), outcome: StageOutcome::Continue })
            .add(RecordingStage { name: "spf", counter: counter.clone(), outcome: StageOutcome::Continue })
            .add(RecordingStage { name: "dkim", counter, outcome: StageOutcome::Continue })
            .build();
        let names: Vec<&str> = p.stage_names().collect();
        assert_eq!(names, vec!["rate-limit", "ptr", "spf", "dkim"]);
    }

    #[tokio::test]
    async fn spam_threshold_set_before_stages_still_applies() {
        // Builder allows interleaving spam_threshold() and add() calls.
        let p = Pipeline::builder()
            .spam_threshold(2.0) // set first
            .add(ScoringStage(2.5))
            .build();
        assert_eq!(p.spam_threshold(), 2.0);
        let mut c = ctx();
        c.auth_results.dmarc_policy = DmarcPolicy::Pass;
        // 2.5 >= 2.0 → Junk
        match p.run(&mut c).await {
            DeliveryDecision::Junk { .. } => {}
            other => panic!("expected Junk, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn spam_threshold_set_twice_takes_last_value() {
        let p = Pipeline::builder()
            .spam_threshold(2.0)
            .spam_threshold(10.0) // overrides 2.0
            .build();
        assert_eq!(p.spam_threshold(), 10.0);
    }

    #[tokio::test]
    async fn first_stage_decide_skips_all_other_stages() {
        // Decide on the first stage skips everything else.
        let counter = Arc::new(AtomicUsize::new(0));
        let p = Pipeline::builder()
            .add(RecordingStage {
                name: "first-decides",
                counter: counter.clone(),
                outcome: StageOutcome::Decide(DeliveryDecision::Greylist),
            })
            .add(RecordingStage {
                name: "never",
                counter: counter.clone(),
                outcome: StageOutcome::Continue,
            })
            .add(RecordingStage {
                name: "also-never",
                counter: counter.clone(),
                outcome: StageOutcome::Continue,
            })
            .build();
        let mut c = ctx();
        assert_eq!(p.run(&mut c).await, DeliveryDecision::Greylist);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn last_stage_decide_short_circuits_default_branch() {
        // Even on the last stage, Decide overrides make_delivery_decision.
        let counter = Arc::new(AtomicUsize::new(0));
        let p = Pipeline::builder()
            .add(RecordingStage {
                name: "first",
                counter: counter.clone(),
                outcome: StageOutcome::Continue,
            })
            .add(RecordingStage {
                name: "last-decides",
                counter: counter.clone(),
                outcome: StageOutcome::Decide(DeliveryDecision::Greylist),
            })
            .build();
        let mut c = ctx();
        // Default branch would Accept; the last-stage Decide must win.
        assert_eq!(p.run(&mut c).await, DeliveryDecision::Greylist);
    }
}
