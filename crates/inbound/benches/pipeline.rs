//! Microbenchmarks for the inbound pipeline framework.
//!
//! Two suites:
//!
//! - `decision` — pure final-decision policy + Authentication-Results header
//!   construction. Both run once per inbound message between SMTP DATA and
//!   the response code.
//! - `pipeline` — `Pipeline::run` framework dispatch overhead with N no-op
//!   stages. Isolates the executor cost from the actual stage backend cost
//!   (DNS, ClamAV, LLM) — useful as a regression baseline if anyone touches
//!   the dispatcher hot path.
//!
//! Run with `cargo bench -p mailrs-inbound`.

use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr};

use async_trait::async_trait;
use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_inbound::{
    AuthResult, AuthResults, DeliveryDecision, DmarcPolicy, Pipeline, PipelineInput,
    ReceiveContext, Stage, StageOutcome, build_auth_header, format_auth_results_header,
    make_delivery_decision,
};

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
        from_addr: String::new(),
        recipient_whitelist: std::collections::HashSet::new(),
        recipient_blacklist: std::collections::HashSet::new(),
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

fn bench_decision(c: &mut Criterion) {
    let mut group = c.benchmark_group("decision");

    let accept_input = default_input();
    group.bench_function("make_delivery_decision_accept", |b| {
        b.iter(|| make_delivery_decision(black_box(&accept_input)))
    });

    let mut junk_input = default_input();
    junk_input.content_score = 4.0;
    junk_input.ptr_score = 1.5; // total 5.5 ≥ 5.0
    group.bench_function("make_delivery_decision_junk", |b| {
        b.iter(|| make_delivery_decision(black_box(&junk_input)))
    });

    let mut reject_input = default_input();
    reject_input.auth.dmarc_policy = DmarcPolicy::Reject;
    reject_input.auth.dmarc = "fail".into();
    group.bench_function("make_delivery_decision_dmarc_reject", |b| {
        b.iter(|| make_delivery_decision(black_box(&reject_input)))
    });

    let mut greylist_input = default_input();
    greylist_input.greylisted = true;
    group.bench_function("make_delivery_decision_greylist", |b| {
        b.iter(|| make_delivery_decision(black_box(&greylist_input)))
    });

    group.finish();
}

fn bench_auth_header(c: &mut Criterion) {
    let mut group = c.benchmark_group("auth_header");

    group.bench_function("build_auth_header_no_reason", |b| {
        b.iter(|| {
            build_auth_header(
                black_box("mx.example.com"),
                black_box("pass"),
                black_box("pass"),
                black_box("none"),
                black_box("pass"),
                black_box(None),
            )
        })
    });

    group.bench_function("build_auth_header_with_reason", |b| {
        b.iter(|| {
            build_auth_header(
                black_box("mx.example.com"),
                black_box("pass"),
                black_box("fail"),
                black_box("none"),
                black_box("fail"),
                black_box(Some("policy=reject")),
            )
        })
    });

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
    group.bench_function("format_auth_results_header_quadruple", |b| {
        b.iter(|| format_auth_results_header(black_box("mx.example.com"), black_box(&results)))
    });

    group.finish();
}

fn bench_context(c: &mut Criterion) {
    let ctx = default_ctx();
    c.bench_function("receive_context_to_pipeline_input", |b| {
        b.iter(|| ctx.to_pipeline_input(black_box(5.0)))
    });
}

// ===== Pipeline framework dispatch =====
//
// Each stage is a no-op `Continue`. The bench measures the
// executor's cost — `Box<dyn Stage>` dispatch + the final
// `make_delivery_decision` call. Real stages (DNS, ClamAV, LLM)
// dominate any production pipeline; this isolates the framework
// floor below them.

struct NoopStage(&'static str);

#[async_trait]
impl Stage for NoopStage {
    fn name(&self) -> &str {
        self.0
    }
    async fn evaluate(&self, _ctx: &mut ReceiveContext) -> StageOutcome {
        StageOutcome::Continue
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

struct EarlyRejectStage;

#[async_trait]
impl Stage for EarlyRejectStage {
    fn name(&self) -> &str {
        "early-reject"
    }
    async fn evaluate(&self, _ctx: &mut ReceiveContext) -> StageOutcome {
        StageOutcome::Decide(DeliveryDecision::Reject {
            code: 550,
            message: "5.7.1 blocked by policy".into(),
        })
    }
}

fn bench_pipeline_run(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let mut group = c.benchmark_group("pipeline_run");

    // 4 no-op stages — typical floor.
    let pipeline_4 = Pipeline::builder()
        .add(NoopStage("rate-limit"))
        .add(NoopStage("ptr"))
        .add(NoopStage("spf"))
        .add(NoopStage("dkim"))
        .build();
    group.bench_function("4_noop_stages", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut ctx = default_ctx();
                ctx.auth_results.dmarc_policy = DmarcPolicy::Pass;
                pipeline_4.run(black_box(&mut ctx)).await
            })
        })
    });

    // Realistic mix: 4 noop + 2 scoring + final decision.
    let pipeline_mix = Pipeline::builder()
        .add(NoopStage("rate-limit"))
        .add(NoopStage("ptr"))
        .add(NoopStage("spf"))
        .add(NoopStage("dkim"))
        .add(ScoringStage(1.0))
        .add(ScoringStage(0.5))
        .build();
    group.bench_function("realistic_mix_6_stages", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut ctx = default_ctx();
                ctx.auth_results.dmarc_policy = DmarcPolicy::Pass;
                pipeline_mix.run(black_box(&mut ctx)).await
            })
        })
    });

    // Early-reject short-circuit — only first two stages run.
    let pipeline_reject = Pipeline::builder()
        .add(NoopStage("rate-limit"))
        .add(EarlyRejectStage)
        .add(NoopStage("never-runs-1"))
        .add(NoopStage("never-runs-2"))
        .build();
    group.bench_function("early_reject_short_circuit", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut ctx = default_ctx();
                pipeline_reject.run(black_box(&mut ctx)).await
            })
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_decision,
    bench_auth_header,
    bench_context,
    bench_pipeline_run
);
criterion_main!(benches);
