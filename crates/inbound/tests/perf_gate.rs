//! Performance regression gates. See [BUDGETS.md](../BUDGETS.md).
//!
//! Every gated path here runs once per inbound message. The pipeline calls
//! `make_delivery_decision` (which calls `build_auth_header` internally) and
//! `ReceiveContext::to_pipeline_input` exactly once per receive transaction
//! — they sit on the hot path between SMTP DATA and the response code.

use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mailrs_inbound::{
    build_auth_header, format_auth_results_header, make_delivery_decision, AuthResult, AuthResults,
    DmarcPolicy, Pipeline, PipelineInput, ReceiveContext, Stage, StageOutcome,
};

const ITERS: usize = 100;

fn time_median<F: FnMut()>(mut op: F) -> Duration {
    let mut samples = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let start = Instant::now();
        op();
        samples.push(start.elapsed());
    }
    samples.sort();
    samples[ITERS / 2]
}

fn default_input() -> PipelineInput {
    PipelineInput {
        greylisted: false,
        auth: AuthResults {
            spf: "pass".into(),
            dkim: "pass".into(),
            arc: "none".into(),
            dmarc: "pass".into(),
            dmarc_policy: DmarcPolicy::Pass,
        },
        virus_found: None,
        content_score: 1.5,
        matched_rules: vec!["missing_from".into(), "html_only_no_text".into()],
        ptr_score: 0.5,
        ai_score: 0.0,
        spam_threshold: 5.0,
        hostname: "mx.example.com".into(),
    }
}

fn default_ctx() -> ReceiveContext {
    ReceiveContext::new(
        IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
        "client.example.com",
        "alice@example.com",
        "bob@example.com",
        b"From: alice@example.com\r\nSubject: Hi\r\n\r\nhello".to_vec(),
        "mx.example.com",
    )
}

#[test]
fn make_delivery_decision_accept_under_budget() {
    let input = default_input();
    let median = time_median(|| {
        let _ = make_delivery_decision(&input);
    });
    // Budget: 30 µs (~30× headroom). Observed P95 (dev): ~1.1 µs.
    let budget = Duration::from_micros(30);
    assert!(
        median < budget,
        "make_delivery_decision (Accept) median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn make_delivery_decision_junk_under_budget() {
    let mut input = default_input();
    input.content_score = 4.0;
    input.ptr_score = 1.5; // total 5.5 ≥ 5.0
    let median = time_median(|| {
        let _ = make_delivery_decision(&input);
    });
    // Budget: 50 µs (~30× headroom). Observed P95 (dev): ~1.8 µs. Junk path
    // has extra format! for the reason string with score breakdown + rules.
    let budget = Duration::from_micros(50);
    assert!(
        median < budget,
        "make_delivery_decision (Junk) median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn make_delivery_decision_dmarc_reject_under_budget() {
    let mut input = default_input();
    input.auth.dmarc_policy = DmarcPolicy::Reject;
    input.auth.dmarc = "fail".into();
    let median = time_median(|| {
        let _ = make_delivery_decision(&input);
    });
    // Budget: 30 µs (~25× headroom). Observed P95 (dev): ~1.3 µs. Reject
    // path still builds the auth_header even though it's not returned.
    let budget = Duration::from_micros(30);
    assert!(
        median < budget,
        "make_delivery_decision (Reject) median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn build_auth_header_under_budget() {
    let median = time_median(|| {
        let _ = build_auth_header("mx.example.com", "pass", "pass", "none", "pass", None);
    });
    // Budget: 20 µs (~20× headroom). Observed P95 (dev): ~1.1 µs.
    let budget = Duration::from_micros(20);
    assert!(
        median < budget,
        "build_auth_header median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn build_auth_header_with_reason_under_budget() {
    let median = time_median(|| {
        let _ = build_auth_header(
            "mx.example.com",
            "pass",
            "fail",
            "none",
            "fail",
            Some("policy=reject"),
        );
    });
    // Budget: 20 µs (~15× headroom). Observed P95 (dev): ~1.3 µs.
    let budget = Duration::from_micros(20);
    assert!(
        median < budget,
        "build_auth_header (with reason) median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn format_auth_results_header_quadruple_under_budget() {
    let results = vec![
        AuthResult {
            method: "spf".into(),
            result: "pass".into(),
            reason: None,
        },
        AuthResult {
            method: "dkim".into(),
            result: "pass".into(),
            reason: None,
        },
        AuthResult {
            method: "arc".into(),
            result: "none".into(),
            reason: None,
        },
        AuthResult {
            method: "dmarc".into(),
            result: "pass".into(),
            reason: None,
        },
    ];
    let median = time_median(|| {
        let _ = format_auth_results_header("mx.example.com", &results);
    });
    // Budget: 20 µs (~30× headroom). Observed P95 (dev): ~0.7 µs.
    let budget = Duration::from_micros(20);
    assert!(
        median < budget,
        "format_auth_results_header median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn receive_context_to_pipeline_input_under_budget() {
    let ctx = default_ctx();
    let median = time_median(|| {
        let _ = ctx.to_pipeline_input(5.0);
    });
    // Budget: 20 µs. Release P95 ~125 ns (≈30× headroom on release);
    // dev mode allocates ~5 short Strings and sub-µs scheduler noise
    // edges over a 5 µs budget intermittently. 20 µs still catches
    // order-of-magnitude regressions.
    let budget = Duration::from_micros(20);
    assert!(
        median < budget,
        "ReceiveContext::to_pipeline_input median {median:?} exceeded {budget:?}"
    );
}

// ===== Pipeline::run dispatch overhead =====
//
// Measures the framework cost of running a Pipeline end-to-end, isolated
// from real stage work. Each stage is a no-op `Continue`; the only thing
// this gate measures is the async dispatch + final
// `make_delivery_decision` call. Real-world `Pipeline::run` cost is
// dominated by the stage backends (DNS, ClamAV, LLM) and is owned by
// downstream consumers — not this crate.

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

#[tokio::test]
async fn pipeline_run_dispatch_overhead_under_budget() {
    let pipeline = Pipeline::builder()
        .add(NoopStage)
        .add(NoopStage)
        .add(NoopStage)
        .add(NoopStage)
        .build();

    let mut samples = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let mut ctx = default_ctx();
        let start = Instant::now();
        let _ = pipeline.run(&mut ctx).await;
        samples.push(start.elapsed());
    }
    samples.sort();
    let median = samples[ITERS / 2];

    // Budget: 100 µs (~30× headroom). Observed P95 (dev): ~3 µs for 4
    // no-op stages + the final make_delivery_decision call (which itself
    // is ~1 µs). The async dispatch through `Box<dyn Stage>` adds a few
    // hundred nanoseconds per stage. This budget ensures any future
    // framework-level regression (e.g. allocating per-stage on hot path,
    // adding a Mutex around shared state) is caught.
    let budget = Duration::from_micros(100);
    assert!(
        median < budget,
        "Pipeline::run dispatch median {median:?} exceeded {budget:?}"
    );
}
