# mailrs-inbound

[![Crates.io](https://img.shields.io/crates/v/mailrs-inbound?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-inbound)
[![docs.rs](https://img.shields.io/docsrs/mailrs-inbound?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-inbound)
[![License](https://img.shields.io/crates/l/mailrs-inbound?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-inbound?style=flat-square)](https://crates.io/crates/mailrs-inbound)

Composable SMTP receive pipeline framework for Rust mail servers. Define
your checks as [`Stage`] implementations, compose them with the
[`Pipeline`] builder, and let early-rejection short-circuit the rest of
the chain when one stage decides. The pure [`make_delivery_decision`]
policy combines accumulated signals into a final [`DeliveryDecision`].

Extracted from [mailrs] so any Rust SMTP server can lean on the same
multi-stage receive pattern that fronts a production-tested mail
infrastructure — without inheriting opinions about which DKIM verifier,
which greylist backend, which virus scanner, or which scoring model to use.

This is, at the time of writing, the **only standalone SMTP receive
pipeline framework on crates.io**.

## Highlights

- **Backend-free** — the crate ships zero protocol code. No SPF / DKIM
  verifier, no DNSBL lookup, no ClamAV protocol, no LLM provider. Wrap
  whichever crate you prefer behind your own [`Stage`] impl.
- **Single-method trait** — every stage exposes one `async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome`. Trait objects work, composition is cheap, testing is trivial.
- **Early reject** — first `StageOutcome::Decide(_)` short-circuits the
  pipeline. No wasted virus-scan on a greylist-deferred message.
- **Pure decision combiner** — [`make_delivery_decision`] takes a
  [`PipelineInput`] (struct of signals) and returns a
  [`DeliveryDecision`]. Pure function, no async, no I/O — call it
  directly if you have your own orchestration.
- **RFC 8601 auth-header helpers** — [`build_auth_header`] /
  [`format_auth_results`] / [`AuthResult`] build the
  `Authentication-Results:` header used by every modern mail server.

## How it slots together

```text
              ┌──── stages run in order, mutating ReceiveContext ─────┐
              │                                                       │
ReceiveContext ──> [GreylistStage] ──> [DkimStage] ──> [ClamavStage] ──> ...
              │       │                  │                │            │
              │       Continue           Continue         Decide(...)  │
              │       (writes signal)    (writes signal)  (terminal)   │
              ▼                                                        ▼
                                                              DeliveryDecision
                                                              (Accept / Junk /
                                                               Reject / Greylist)
```

If every stage returns `Continue`, [`Pipeline::run`] calls
[`make_delivery_decision`] over the accumulated signals to produce the
final decision.

## Quick start

```rust,ignore
use async_trait::async_trait;
use mailrs_inbound::{Pipeline, ReceiveContext, Stage, StageOutcome};
use std::net::{IpAddr, Ipv4Addr};

// Your own check — wraps whatever backend you like.
struct GreylistStage { /* your greylist db, config, ... */ }

#[async_trait]
impl Stage for GreylistStage {
    fn name(&self) -> &str { "greylist" }
    async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
        // ... look up the (ip, sender, recipient) triplet in your store ...
        // if first time: return StageOutcome::Decide(DeliveryDecision::Greylist);
        // otherwise:     mark ctx.greylisted = false and return Continue
        StageOutcome::Continue
    }
}

# async fn run() {
let pipeline = Pipeline::builder()
    .add(GreylistStage { /* ... */ })
    // ... more stages: SPF check, DKIM verify, ClamAV scan, content scoring, etc.
    .spam_threshold(8.0)
    .build();

let mut ctx = ReceiveContext::new(
    IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
    "client.example.com",
    "alice@example.com",
    "bob@example.com",
    b"From: alice...".to_vec(),
    "mx.example.com",
);

let decision = pipeline.run(&mut ctx).await;
// match decision { ... handle Accept / Junk / Reject / Greylist }
# let _ = decision;
# }
```

## Example stage shapes

These are **examples**, not bundled implementations — you write them in
your own server crate.

```rust,ignore
// SPF / DKIM / DMARC via mail-auth — mailrs's own production stage:
struct MailAuthStage {
    authenticator: Arc<MessageAuthenticator>,
    hostname: String,
}

#[async_trait]
impl Stage for MailAuthStage {
    fn name(&self) -> &str { "mail_auth" }
    async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
        let spf_params = SpfParameters::verify_mail_from(
            ctx.client_ip, &ctx.ehlo_domain, &self.hostname, &ctx.sender,
        );
        let spf_output = self.authenticator.verify_spf(spf_params).await;
        ctx.auth_results.spf = spf_token(spf_output.result());
        // ... DKIM + ARC + DMARC ...
        StageOutcome::Continue
    }
}

// ClamAV TCP virus scan:
struct ClamavStage { addr: String }
#[async_trait]
impl Stage for ClamavStage {
    fn name(&self) -> &str { "clamav" }
    async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
        if let Some(sig) = scan(&self.addr, &ctx.message).await {
            ctx.virus_found = Some(sig);
            // Don't decide here — let make_delivery_decision turn it into a Reject.
        }
        StageOutcome::Continue
    }
}
```

## Default decision policy

If every stage returns `Continue`, the final decision comes from
[`make_delivery_decision`] which evaluates signals in this precedence
order (high → low):

1. **Greylist** (highest precedence — defer before any other work).
2. **Virus found** → hard 550 reject.
3. **DMARC `p=reject`** → hard 550 reject.
4. **DMARC `p=quarantine`** → Junk.
5. **`content_score + ptr_score + ai_score >= spam_threshold`** → Junk.
6. **Default**: Accept.

The function is **pure** — same input always produces the same output.
Use it directly if you'd rather orchestrate the pipeline yourself:

```rust,no_run
use mailrs_inbound::{
    make_delivery_decision, AuthResults, DmarcPolicy, PipelineInput,
};

let input = PipelineInput {
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
    matched_rules: vec![],
    ptr_score: 0.0,
    ai_score: 0.5,
    spam_threshold: 5.0,
    hostname: "mx.example.com".into(),
};

let decision = make_delivery_decision(&input);
```

## What's NOT in the crate (intentionally)

- No SPF / DKIM / ARC / DMARC verifier — pick your favorite Rust crate.
- No DNS resolver type — stages get whichever resolver they want.
- No virus scanner — ClamAV, rspamd, etc.
- No greylist backend — Redis / Memcached / Postgres / in-memory.
- No LLM / ML scoring provider.
- No DMARC reporting — separate concern.
- No `Authentication-Results:` parser (consumer-side reading of headers from prior hops) — the crate emits, doesn't ingest.

These all live as concrete [`Stage`] implementations in your own crate.
This keeps the framework dependency-light and free of opinion.

## Tested

`1.0.0` ships **37 unit tests** across 5 modules covering every method:

| Module | Tests | Surface |
| --- | ---: | --- |
| `auth_header` | 11 | RFC 8601 formatting — single result, multi-result folding, reasons, temperror / permerror passthrough, `build_auth_header` quadruple |
| `decision` | 12 | Precedence ordering, every variant, score thresholds, auth-header carried through |
| `context` | 4 | `ReceiveContext::new` initialization, `to_pipeline_input` round-trip |
| `stage` | 3 | Trait object shape, NoopStage / AlwaysRejectStage |
| `pipeline` | 7 | Empty pipeline, sequential execution, early-reject short-circuit, signal mutation, custom threshold |

Run with `cargo test -p mailrs-inbound`.

## Performance

[`benches/pipeline.rs`](benches/pipeline.rs) covers the four hot paths the SMTP DATA → response handler hits per message: final-decision policy, Authentication-Results building, `ReceiveContext` materialization, and `Pipeline::run` dispatch overhead with no-op stages (real stages dominate any production pipeline; this measures the framework floor).

Measured with criterion 0.8 on Apple Silicon (M-series), `cargo bench`, release profile.

| Operation | Median | Notes |
|---|---|---|
| `make_delivery_decision` (Accept) | ~360 ns | builds + carries the auth header |
| `make_delivery_decision` (Junk) | ~850 ns | extra `format!` for the reason string with score breakdown |
| `make_delivery_decision` (DMARC reject) | ~465 ns | reject path still builds auth header for logging |
| `make_delivery_decision` (Greylist) | ~3 ns | early-exit short-circuit |
| `build_auth_header(no reason)` | ~380 ns | one allocation, four short string interpolations |
| `build_auth_header(with reason)` | ~425 ns | + reason parenthetical |
| `format_auth_results_header(quadruple)` | ~280 ns | flatten 4 `AuthResult`s into one header line |
| `ReceiveContext::to_pipeline_input(5.0)` | ~200 ns | clones AuthResults + matched_rules + hostname |
| `Pipeline::run` (4 no-op stages + decide) | ~600 ns | full async dispatch path |
| `Pipeline::run` (4 noop + 2 scoring + decide) | ~635 ns | realistic stage mix |
| `Pipeline::run` (early-reject after 2 stages) | ~185 ns | short-circuit on `StageOutcome::Decide` |

Run with `cargo bench -p mailrs-inbound`. See [`tests/perf_gate.rs`](tests/perf_gate.rs) for the regression budgets — `Pipeline::run` dispatch is gated at 100 µs, with plenty of headroom over the ~600 ns measurement above.

## Versioning

`1.x` follows semver. The stable public surface:

- `Stage` trait method signatures
- `Pipeline` + `PipelineBuilder` method signatures
- `ReceiveContext` (marked `#[non_exhaustive]` so we can add signal
  fields in minor versions without breaking destructure patterns)
- `DeliveryDecision`, `AuthResults`, `DmarcPolicy`, `StageOutcome` enum variants
- `PipelineInput` struct shape + `make_delivery_decision` signature
- All public functions in `auth_header::*`

The default policy in `make_delivery_decision` may evolve within `1.x` if
the precedence rules need tightening; consumers who want to lock it in
should compute their own final decision from the signals.

## License

Licensed under either [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

[mailrs]: https://github.com/goliajp/mailrs
[`Stage`]: https://docs.rs/mailrs-inbound/latest/mailrs_inbound/stage/trait.Stage.html
[`Pipeline`]: https://docs.rs/mailrs-inbound/latest/mailrs_inbound/pipeline/struct.Pipeline.html
[`make_delivery_decision`]: https://docs.rs/mailrs-inbound/latest/mailrs_inbound/decision/fn.make_delivery_decision.html
[`PipelineInput`]: https://docs.rs/mailrs-inbound/latest/mailrs_inbound/decision/struct.PipelineInput.html
[`DeliveryDecision`]: https://docs.rs/mailrs-inbound/latest/mailrs_inbound/decision/enum.DeliveryDecision.html
[`build_auth_header`]: https://docs.rs/mailrs-inbound/latest/mailrs_inbound/auth_header/fn.build_auth_header.html
[`format_auth_results`]: https://docs.rs/mailrs-inbound/latest/mailrs_inbound/auth_header/fn.format_auth_results.html
[`AuthResult`]: https://docs.rs/mailrs-inbound/latest/mailrs_inbound/auth_header/struct.AuthResult.html
