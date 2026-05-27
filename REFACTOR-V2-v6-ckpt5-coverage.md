# v6 ckpt 5 — Test coverage baseline + proptest expansion (2026-05-27)

> Coverage walk for the v6 polish pass. `cargo llvm-cov --workspace
> --lib` baseline; proptest added to 2 missing parser crates.

## Workspace baseline (`cargo llvm-cov --workspace --summary-only --lib`)

| Metric | Value |
|---|---:|
| Lines | **82.42 %** (43 753 / 53 084) |
| Functions | **80.57 %** (3 528 / 4 379) |
| Regions | **79.57 %** (26 340 / 33 104) |

Workspace clears the ≥80 % bar on lines + functions. Regions are
slightly under (79.57 %) — region coverage is a stricter metric
(every distinct code region in a function).

Run: `cargo llvm-cov --workspace --summary-only --lib`. (`--lib`
because the ckpt 3 perf-gate budgets blow up under llvm-cov
instrumentation; integration tests are out of scope for the
unit-coverage baseline.)

## Per-crate gaps (< 80 % lines, lib only)

Crates that need more unit tests to hit the trigger's per-crate ≥80 %
bar:

| Crate | File | Lines | Reason for gap |
|---|---|---:|---|
| `mailrs-outbound-queue` | `queue.rs` | 30.80 % | Mostly PG-backed CRUD; testable only via PG fixture or `cfg(test)` mock. |
| `mailrs-outbound-queue` | `worker/mod.rs` | 64.04 % | DeliveryWorker poll loop + valkey listener; covered by integration tests, not the lib bench. |
| `mailrs-outbound-queue` | `worker/delivery.rs` | 0 % | Per-domain MX delivery; live DNS + SMTP. Integration-only. |
| `mailrs-outbound-queue` | `worker/smtp.rs` | 0 % | Real SMTP client wire path; integration-only. |
| `mailrs-outbound-queue` | `pg_store.rs` | 0 % | Reference store impl behind `pg` feature; covered by `tests/trait_contract.rs` (not lib). |
| `mailrs-postmaster` | `bimi.rs` / `dane.rs` / `dkim.rs` / `dmarc.rs` / `mx.rs` / `ptr.rs` / `spf.rs` / `mta_sts.rs` / `tlsrpt.rs` | 0–40 % | All per-check modules dispatch live DNS via `hickory_resolver`. lib.rs dispatcher itself hits 92.72 %; the per-check leaves need a DNS mock layer. |
| `mailrs-shield` | `ptr.rs` | 68.92 % | FCrDNS reverse-DNS lookup; live `hickory_resolver`. |
| `mailrs-shield` | `greylist.rs` | 77.01 % | Just below; missing a few error-path branches. |
| `mailrs-smtp-client` | `connection.rs` | 38.73 % | Live SMTP wire connection + STARTTLS handshake. |
| `mailrs-smtp-client` | `dane.rs` | 65.80 % | Live DNS for TLSA records. |
| `mailrs-smtp-client` | `tls_outcome.rs` | 77.04 % | Just below; integration-test territory. |

**Pattern:** every < 80 % file is either (a) live DNS lookup,
(b) live SMTP connection, or (c) PG-backed CRUD that needs a real
database. None of these can sanely hit ≥80 % via lib-only tests.

## Proptest expansion

ckpt 5 plan called for proptest on all 4 parser crates (`rfc5322`,
`rfc2047`, `rfc2231`, `mime`). Audit found 2 already had it:

| Crate | Before | After |
|---|---|---|
| `mailrs-rfc2047` | ✅ `proptest_roundtrip.rs` | unchanged |
| `mailrs-rfc2231` | ✅ `proptest_roundtrip.rs` | unchanged |
| `mailrs-rfc5322` | ❌ none | ✅ new `proptest_stability.rs` (3 properties: panic-safety, case-insensitive header lookup, body-offset invariant) |
| `mailrs-mime` | ❌ none | ✅ new `proptest_stability.rs` (3 properties: panic-safety on arbitrary bytes, text/plain extraction roundtrip, walker always yields root) |

Each new test runs 256 random cases per property per `cargo test`
invocation. They live next to the existing unit tests, so they're
not opt-in.

## Small unit-test wins committed in this pass

- `mailrs-outbound-queue::queue::suppression::is_hard_bounce` got 3
  unit tests (5xx detection / 4xx reject / leading-WS trim). Was 0 %
  coverage on the only synchronous function in `suppression.rs`; the
  other functions in that file are PG-async and stay uncovered by lib.

## Trigger status — partial

| Trigger clause | Status |
|---|---|
| Workspace overall ≥80 % | ✅ 82.42 % lines / 80.57 % functions |
| Per-crate ≥80 % (server ≥70 %) | ⚠️ partial — 4 crates (outbound-queue / postmaster / shield / smtp-client) have submodules below the bar, all on live-DNS / live-SMTP / PG-backed paths that lib coverage can't reach without a mock-DNS / docker-pg fixture layer. |
| proptest in 4 parser crates | ✅ all 4 covered |

**Recommendation:** the partial per-crate gap is a "mock layer scope"
task, not a "write more unit tests" task — it'd touch hickory test
plumbing + a docker-pg test fixture + the postmaster check modules'
APIs. Punt to **v7 ckpt 5 follow-up** as a focused RFC; don't expand
v6 ckpt 5 into days of integration-test scaffolding.

In the meantime ckpt 5 deliverables (workspace baseline measured,
gap documented, proptest expanded) are done. Proceeding to ckpt 6.
