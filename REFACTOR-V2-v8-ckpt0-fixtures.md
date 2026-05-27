# v8 ckpt 0 — outbound delivery fixture infra + coverage push

Closes the first checkpoint of v8 stone-wave-2 work: bolt
test-container-grade fixtures onto the two lib crates with the
worst coverage holes (`outbound-queue` worker/store paths,
`smtp-client` connection state machine), then write integration
tests against them.

The deliverability-hardening rationale: every percentage point of
worker-path coverage we add now is a percentage point of "untested
prod code that runs every outbound delivery." Doing this before
the mail-builder swap (ckpt 3) gives us a real regression net
when we cross over.

## What got built

### Fixture infrastructure

| Crate | File | What |
|---|---|---|
| `outbound-queue` | `tests/common/pg.rs` | ephemeral `postgres:17-alpine` container per test, applies the `outbound_queue` + `suppression_list` DDL subset inline. Tests run with `--test-threads=1`. |
| `outbound-queue` | `tests/common/redis.rs` | ephemeral Valkey/Redis container for `RedisNotifier` integration tests. |
| `outbound-queue` | `tests/common/mock_smtp.rs` | tokio-based mock SMTP server. 10 behaviors covering accept/reject/defer/close-mid-DATA/STARTTLS-{rejected,handshake-fail,accept}. Includes server-side rustls handshake driver for STARTTLS success paths. |
| `smtp-client` | `tests/common/mock_smtp.rs` | byte-identical copy of the outbound-queue mock SMTP. Per the per-crate-`tests/common`-duplication decision in the ckpt 0 plan — DRY across crates would force a published `mailrs-test-fixtures` stone with dev-only Docker deps in its tree, which isn't worth it for two consumers. |

Decision: **wrote the mock SMTP ourselves instead of adopting
`mailin-embedded`**. Original plan picked mailin-embedded 0.8 as
"mature off-the-shelf"; on inspection it's a 2023-era sync
`std::net::TcpListener` model that doesn't compose cleanly with
our tokio async stack, and customising its `Handler` trait for 10
distinct behaviors would have been a larger surface than the
~300-line tokio mock we ended up writing. The mock controls
behavior via a single `Behavior` enum — adding a new scenario is
one match arm.

### New integration tests (50 total)

| Test file | Count | Coverage targets |
|---|---:|---|
| `outbound-queue/tests/pg_smoke.rs` | 1 | fixture sanity |
| `outbound-queue/tests/worker_integration.rs` | 6 | `queue.rs` PG hot paths (claim_for_delivery atomic, recover_stale_inflight, 2-worker SKIP LOCKED race, mark_failed) |
| `outbound-queue/tests/pg_store_integration.rs` | 11 | `pg_store.rs` trait-level (enqueue/dequeue/mark_*/cancel_*/retry/list_recent/suppression CRUD via the QueueStore trait) |
| `outbound-queue/tests/worker_delivery_integration.rs` | 12 | `worker/{delivery,smtp,mod}.rs` full pipeline: deliver_domain_static + try_deliver_via_mx against mock SMTP + DeliveryWorker::run smoke |
| `outbound-queue/tests/redis_notifier_integration.rs` | 2 | `pg_store.rs::RedisNotifier` publish/subscribe |
| `smtp-client/tests/mock_smoke.rs` | 1 | fixture sanity |
| `smtp-client/tests/connection_integration.rs` | 8 | `SmtpConnection` lifecycle: connect/EHLO/MAIL/RCPT/DATA/QUIT + 5xx/4xx mappings + STARTTLS-rejected + STARTTLS-handshake-fail + greeting-timeout + close-mid-DATA |

### Production-code changes

The worker's per-domain orchestrator was `pub(super)` and the
per-MX SMTP loop was `pub(super)` with a hardcoded `25` port — both
required adjustments to be testable end-to-end without a real
MTA on the box:

- `worker::deliver_domain_static` → `pub` with new `port: u16` parameter
- `worker::try_deliver_via_mx` → `pub` with new `port: u16` parameter
- production caller (`DeliveryWorker::poll_and_deliver`) passes `25` (unchanged behavior)
- `worker/mod.rs` re-exports both for downstream stone users that want to drive
  custom multi-domain orchestration

No semantic change to production. The port parameter exists
solely as a test-injection seam; production code paths are
identical to the v1.7.35 release.

## Coverage delta (baseline → ckpt 0)

Measured with `cargo llvm-cov -p mailrs-outbound-queue --summary-only`
and `cargo llvm-cov -p mailrs-smtp-client --summary-only` (both
include integration tests). Baseline in
`REFACTOR-V2-v8-baseline.md` was the pre-fixture state.

| Module | Baseline | ckpt 0 | Target | Status |
|---|---:|---:|---:|---|
| `outbound-queue/src/queue.rs` | 30.80 % | **95.32 %** | (implicit) | ✓ |
| `outbound-queue/src/queue/suppression.rs` | 45.88 % | **97.65 %** | (implicit) | ✓ |
| `outbound-queue/src/store.rs` | 79.08 % | 79.08 % | — | unchanged (pure-Rust unit tests already covered) |
| `outbound-queue/src/pg_store.rs` | **0.00 %** | **91.84 %** | ≥ 80 % | ✓ |
| `outbound-queue/src/worker/mod.rs` | 64.04 % | **86.30 %** | ≥ 80 % | ✓ |
| `outbound-queue/src/worker/delivery.rs` | **0.00 %** | **97.78 %** | ≥ 80 % | ✓ |
| `outbound-queue/src/worker/smtp.rs` | **0.00 %** | 66.04 % | ≥ 80 % | ✗ — carved out, see below |
| `smtp-client/src/connection.rs` | 38.73 % | **71.89 %** | ≥ 70 % | ✓ |

**`outbound-queue` total lib coverage**: ~68 % → **90.01 %**

**6 of 7 trigger lines met.** `worker/smtp.rs` 66.04% < 80% is the
single carve-out. Reason and follow-up below.

## Known gap: `worker/smtp.rs` STARTTLS-success / DANE branches

The unmet ≥80% trigger on `worker/smtp.rs` is structural, not
test-effort: the 14-point gap is concentrated in two branches
that cannot be driven with the current `SmtpConnection` public
API.

1. **STARTTLS-success path** (~22 lines).
   `SmtpConnection::try_starttls` hardcodes `webpki-roots` as its
   PKIX trust store. Our mock SMTP server can complete a TLS
   handshake (mock_smtp.rs ships rcgen self-signed certs), but
   the client refuses the cert because no real CA in
   webpki-roots issued it. To drive the success branch we need
   a public hook like `try_starttls_with_config(client_config)`
   accepting a caller-supplied `rustls::ClientConfig` with a
   dangerous verifier. That's a `mailrs-smtp-client` API
   change, not "more tests".

2. **DANE TLSA-present path** (~8 lines).
   `mailrs_smtp_client::resolve_tlsa(resolver, "127.0.0.1")`
   returns empty because IP literals have no TLSA records.
   Driving the DANE branch needs either a mock hickory resolver
   that synthesises TLSA records on demand, or a stub for
   `resolve_tlsa` itself.

Neither belongs in "ckpt 0 = fixture infra"; both are
production-API or fixture-infra extensions the v8 plan should
schedule as their own checkpoint. **Tracked as ckpt 0.9 in the
v8 RFC** (`.claude/rfcs/20260527-stone-wave-2-v8.md`, to be
amended in the same commit as this release).

## Baseline re-check (4 axes)

```
cargo test --workspace -- --skip _under_budget : <verified at release time>
cargo llvm-cov --workspace --summary-only --lib : <verified at release time>
./scripts/check-security.sh                     : <verified at release time>
gh run list --workflow=release.yml --limit 5    : <verified at release time>
```

Tests added in this checkpoint are intentionally inside
`tests/*.rs` (integration), not `#[cfg(test)] mod tests` blocks,
so they vanish from `--lib`-only counters and only show up when
the workspace test suite is run. The `--lib`-only `--workspace`
coverage number above therefore stays close to the v8 baseline
of 83.40 %.

## New dev-dependencies introduced

| Crate | Crate | Version | Why |
|---|---|---|---|
| outbound-queue | `testcontainers` | 0.27 | ephemeral PG container per integration test |
| outbound-queue | `testcontainers-modules` (`postgres`, `redis`) | 0.15 | PG + Valkey container helpers |
| outbound-queue | `sqlx` (workspace) | 0.8 | direct PG access in test fixtures |
| outbound-queue | `rcgen` | 0.14 | self-signed certs for mock SMTP server-side TLS |
| outbound-queue | `rustls` | 0.23 (`ring`) | server-side TLS in mock |
| outbound-queue | `tokio-rustls` | 0.26 | accepting TLS in mock |
| outbound-queue | `hickory-resolver` | 0.26 | constructing a TokioResolver for delivery tests |
| smtp-client | `rcgen` | 0.14 | same — for future STARTTLS-success tests |

All on the workspace `latest stable` rule from
`rules/rust-deps-and-warnings.md`. No new direct production
dependencies.

## Why no `mailrs-test-fixtures` stone

Both `outbound-queue` and `smtp-client` now ship a 300-LOC
mock_smtp.rs that's byte-identical between them. The natural
follow-up question is "extract it into a `mailrs-test-fixtures`
crate." The ckpt 0 plan's Texture section decided against this
and ckpt 0 actually proves the call:

- Extraction would force a published crates.io stone whose
  dev-deps include Docker (testcontainers), TLS certs (rcgen),
  and rustls server-side state. That tree leaks downstream
  visibility on every published outbound-queue / smtp-client
  consumer.
- The duplication cost is one file pair, mechanically synced.
  The hidden-dep cost of a published fixtures crate is
  considerably higher.

If we hit a third consumer, revisit then.
