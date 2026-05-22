# Performance — what's measured, what's not

mailrs's positioning is "modern Rust implementation of legacy email
protocols, performance-first". For that to mean anything, every number
that appears in a commit message, README, BUDGETS.md, or blog post
**must trace back to a measurement that anyone can reproduce.** Guesses
don't count. Estimates don't count. Numbers we'd like to be true don't
count.

This file is the source of truth for which mailrs perf claims are
honestly measured and which are still open. When in doubt, default to
the latest column ("Measured?") here — not to whatever a commit message
or marketing material says.

## Measured

### Workspace-level

| Path | Measurement | Run command |
|---|---|---|
| Release binary size (mailrs-server) | 44 MB (default) → 22 MB (perf-first profile). M-series Mac. | `du -h $TARGET_DIR/release/mailrs-server` before/after commit `9f21e0b`. |

### `mailrs-inbound` (criterion bench, M-series Mac, release, 100-sample median ± 95% CI from criterion's own analysis)

| Path | Median | Notes |
|---|---:|---|
| `decision::make_delivery_decision_greylist` | **2.4 ns** | trivial early return |
| `context::receive_context_to_pipeline_input` | **65 ns** | per-message snapshot clone |
| `pipeline_run/early_reject_short_circuit` | **201 ns** | first stage rejects → entire pipeline |
| `auth_header::format_auth_results_header_quadruple` | **228 ns** | RFC 8601 4-method header |
| `decision::make_delivery_decision_accept` | **337 ns** | Accept path + auth header build |
| `auth_header::build_auth_header_no_reason` | **342 ns** | DMARC pass header (no reason) |
| `decision::make_delivery_decision_dmarc_reject` | **408 ns** | Reject path + auth header build (header built even though not returned) |
| `auth_header::build_auth_header_with_reason` | **429 ns** | DMARC fail header with `reason="policy=…"` |
| `pipeline_run/4_noop_stages` | **610 ns** | framework dispatch cost only |
| `pipeline_run/realistic_mix_6_stages` | **648 ns** | dispatch + 6 cheap noop-style stages |
| `decision::make_delivery_decision_junk` | **735 ns** | Junk path — extra `format!` for score-breakdown reason |

Run: `cargo bench -p mailrs-inbound --bench pipeline` (the bench file
ships in `crates/inbound/benches/pipeline.rs`).

### Other crate-level perf gates (regression-catch only)

Each crate's `tests/perf_gate.rs` documents a budget per gated path and
runs as part of `cargo test`. These are *not* publishable numbers (the
gates have 15-30× headroom so they catch order-of-magnitude regressions,
not micro-perf swings). Don't quote them as performance claims; quote
the criterion bench medians above instead.

| Crate | `cargo test -p mailrs-<crate> --test perf_gate` | Gate count |
|---|---|---:|
| mailrs-clean | budgets in `BUDGETS.md` | 3 |
| mailrs-dav | budgets in `BUDGETS.md` | 3 |
| mailrs-dmarc | budgets in `BUDGETS.md` | 2 |
| mailrs-ical | budgets in `BUDGETS.md` | 2 |
| mailrs-imap-proto | budgets in `BUDGETS.md` | 3 |
| mailrs-inbound | budgets in `BUDGETS.md` | 8 |
| mailrs-intelligence | budgets in `BUDGETS.md` | 2 |
| mailrs-jmap | budgets in `BUDGETS.md` | 4 |
| mailrs-mailbox | budgets in `BUDGETS.md` | 8 |
| mailrs-outbound-queue | budgets in `BUDGETS.md` | 6 |
| mailrs-postmaster | budgets in `BUDGETS.md` | 4 |
| mailrs-rate-limit | budgets in `BUDGETS.md` | 4 |
| mailrs-shield | budgets in `BUDGETS.md` | 5 |
| mailrs-smtp-client | budgets in `BUDGETS.md` | 3 |
| mailrs-smtp-proto | budgets in `BUDGETS.md` | 5 |
| mailrs-maildir | budgets in `BUDGETS.md` | 3 |

### `mailrs-smtp-proto` (criterion, `cargo bench -p mailrs-smtp-proto`)

| Path | Median | Notes |
|---|---:|---|
| `parse_command/EHLO` | **22 ns** | hot wire-parse path |
| `parse_command/DATA` | **22 ns** | |
| `parse_command/RCPT_TO` | **70 ns** | envelope address extract |
| `parse_command/MAIL_FROM` | **103 ns** | envelope address extract |
| `format_ehlo_response` | **307 ns** | full multi-line EHLO response build |
| `address/is_valid_typical` | **10 ns** | |
| `address/split_typical` | **12 ns** | |

### `mailrs-smtp-client` (criterion, `cargo bench -p mailrs-smtp-client`)

| Path | Median | Notes |
|---|---:|---|
| `sort_mx_records(20)` | **~12 ns** | MX priority sort |
| `parse_response(short)` | **~30 ns** | 250 OK |
| `parse_response(10-line EHLO)` | **~290 ns** | multi-line response |
| `dot_stuff(5 KB no dots)` | **~1.4 µs** | passthrough fast-path |
| `dot_stuff(5 KB with dots)` | **~1.6 µs** | allocates new Vec to escape |

### `mailrs-imap-proto` (criterion, `cargo bench -p mailrs-imap-proto`)

| Path | Median | Notes |
|---|---:|---|
| `parse_command(LOGIN)` | **~123 ns** | |
| `parse_command(complex UID SEARCH)` | **~155 ns** | |
| `sequence_set_to_uids(4001 uids)` | **~3.0 µs** | range expansion |

### `mailrs-jmap` (criterion, `cargo bench -p mailrs-jmap`)

| Path | Median | Notes |
|---|---:|---|
| `keywords_to_flags` | **~5.6 ns** | bitmask conversion |
| `dispatch Email/query` | **~2.4 µs** | single dispatch w/ in-memory store |
| `dispatch_request multi-call back-ref` | **~10.4 µs** | full JMAP `Request` with `#ref` |

### `mailrs-mailbox` (criterion, `cargo bench -p mailrs-mailbox`)

| Path | Median | Notes |
|---|---:|---|
| `add_flags` hot path | **~55 ns** | DashMap entry update |
| `extract_message_id(short header)` | **~150 ns** | per-message threading helper |
| `query_messages text-match 1k msg` | **~120 µs** | fixture-impl (clones full Message rows — PG impl pushes work into SQL; see README §"Performance") |

### `mailrs-rate-limit` (criterion, `cargo bench -p mailrs-rate-limit`)

| Path | Median | Notes |
|---|---:|---|
| `evaluate_bucket/allowed` (pure math) | **1.7 ns** | f64 arithmetic, no I/O |
| `evaluate_bucket/denied_no_refill` | **1.6 ns** | |
| `check_hot_key/sync` | **33 ns** | bypass async trait |
| `check_hot_key/async` | **84 ns** | through `RateLimitStore` trait |
| `check_cold_key/first_touch` | **~140 ns** | DashMap insert path |
| `cleanup_stale(10k)` | **~100 µs** | batch scan + retain |

### `mailrs-shield` (criterion, `cargo bench -p mailrs-shield`)

| Path | Median | Notes |
|---|---:|---|
| `interpret_spamhaus` | **~700 ps** | bit interpretation of A-record octets |
| `ptr_score_from_names(match)` | **~85 ns** | FCrDNS score eval |
| `triplet_key` | **~120 ns** | greylist key build |

### `mailrs-clean` (criterion, `cargo bench -p mailrs-clean`)

| Path | Median | Notes |
|---|---:|---|
| `clean_email_html(5 KB marketing)` | **~336 µs** | typical-size hot path |
| `clean_email_html(50 KB worst-case)` | **~2.5 ms** | **~22 MB/s** throughput |

### Server-internal (`mailrs-server`, gated `#[test]` bench)

| Path | Measurement | Run command |
|---|---|---|
| `extract_subject_and_from` vs. two `extract_header` calls | Single-pass wins **48-50%** across 1KB/5KB/20KB messages (release). Absolute: saves **2.0 / 3.1 / 6.5 µs** per message respectively. | `MAILRS_BENCH=1 cargo test --release -p mailrs-server bench_two_pass_vs_single_pass -- --nocapture --test-threads=1` |

### Variance note

All numbers above are **criterion 100-sample median on a single M-series
Mac running release profile**. Re-running on the same machine within
minutes typically lands within ±5% of these medians; under heavy
concurrent load (a build going at the same time) sub-µs-scale benches
can swing ±30%. Order-of-magnitude is stable; sub-nanosecond comparisons
between two paths should always be re-measured on the consumer's own
hardware.

### Surfaced potential perf candidates (logged, not yet acted on)

1. `mailbox::InMemoryMailboxStore::query_messages` is ~120 µs for 1k
   messages because the fixture clones every matching `Message`
   (12+ String fields each). The PG impl pushes the work into SQL.
   Acceptable as fixture; flagged in README §"Performance".
2. `inbound::make_delivery_decision(Junk)` is ~2.4× slower than Accept
   entirely due to the `format!` for the score-breakdown reason. If
   Junk traffic becomes a hot percentage, build the reason lazily.
3. `smtp-client::dot_stuff(body_with_dots)` allocates a new `Vec<u8>`;
   the no-dot fast path returns the input slice unchanged. Trade-off
   noted; absolute cost (~1.6 µs for 5 KB) is small enough to defer.

## NOT measured (claims to retract or qualify)

These appeared in commit messages but were guesses. They are NOT
performance claims mailrs stands behind.

### Commit `9f21e0b` (perf-first release profile)

The commit message said "Conservative estimate: +10-20% throughput on
hot paths from cross-crate inlining alone, more on Result-heavy code
paths from panic=abort." **Not measured.** End-to-end mailrs-server
throughput before vs. after the profile change has not been
benchmarked. The binary size delta (-50%) IS real and reproducible;
the throughput delta is plausible but unsubstantiated.

To upgrade this to "measured": run a sustained SMTP-receive benchmark
(e.g. a `smtp-source`-style load generator at 1000 msg/sec) against
`mailrs-server` built with both profiles, compare 99th-percentile
delivery latency.

### Commit `501dd5e` (zero-alloc header scan)

The commit message said "~30-50% allocation reduction on the
header-extract hot path." **Not measured.** The number was an intuition
based on counting allocations in the diff (the fallback path went from
multiple `String` allocations down to zero on miss + one on match), not
a measured allocation profile (e.g. via `dhat` or jemalloc stats).

The structural improvement is real (fewer allocations on the byte-scan
fallback path), but the percent figure is unverified. The fallback path
also only runs when `mail_parser` returns `None` — which is rare.

To upgrade this to "measured": instrument `extract_header` calls with
`dhat::Profiler`, run a representative SMTP receive workload, compare
allocation totals before vs. after the commit.

### Commit `69beb4b` (pre-size recipient Vecs)

Commit message did not claim a percentage. The change is structurally
correct (avoids the geometric resize cascade) but the absolute impact
depends on recipient count distribution, which is not measured. For
typical 1-3 recipient messages the difference is below measurement
noise; for 50+ recipient bulk-mail it should be observable but isn't
gated by a benchmark yet.

## How to add a new perf claim

1. Write a benchmark. Either a criterion bench under `crates/<x>/benches/`
   (slow but rich output, run with `cargo bench`), or a gated `#[test]`
   harness like `bench_two_pass_vs_single_pass_extract` (fast, runs
   in `cargo test --release` with an env gate).
2. Run it. Capture the actual numbers (median over 100+ iterations).
3. Add the number to this file's "Measured" table with the exact
   `cargo` command to reproduce.
4. The commit message can then reference the number — and only the
   number that's in this table.
5. If the optimization is on the hot path and we want CI to catch
   regressions, also promote it to a `tests/perf_gate.rs` row with
   a budget at 15-30× headroom.

## What this discipline protects

The single worst failure mode for a "performance-first" project is
this: someone reads our commit history / README / blog, decides to use
us because of the perf claims, deploys, discovers the claims don't
hold under their workload. The reputational cost is asymmetric —
losing trust is much easier than rebuilding it.

So: every number in this file is a number you can reproduce. Every
number outside this file (in commit messages, READMEs, blog posts)
must point back to a row here. If it doesn't, treat it as folklore
and demand a measurement.
