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

## v4 baseline (2026-06-02, ckpt 0)

Full-workspace `cargo bench --workspace` snapshot taken at commit
`f76c8d4`. Serves as the diff anchor every subsequent v4 stone-ckpt
will measure against. Per the v4 RFC
(`.claude/rfcs/20260602-v4-perf-squeeze.md`), each stone-ckpt compares
its post-optimization numbers to this baseline; drift > 10 % flags an
investigation.

**Environment fingerprint**

- Host: `Mac16,11` (M4 Mac mini)
- OS: macOS 26.5 (Darwin 25.5.0 arm64)
- rustc: 1.96.0 (ac68faa20 2026-05-25)
- samply: 0.13.1
- git HEAD: `f76c8d4`
- Profile: `release` (workspace default — fat LTO + cgu=1)

**Raw artifacts** (local-only, `.claude/` gitignored): full log saved
at `/tmp/v4-baseline-20260602-1926.log` (380 KB, 6939 lines, 309
criterion rows across 38 stones); per-stone JSON dump at
`/tmp/v4-baseline-per-stone.json`.

**Per-stone bench inventory at baseline**

309 criterion bench rows total across **38 stones**; 5 stones (`acme`,
`dns`, `tls-reload`, `mail-builder`, `sieve-core`) have no `benches/`
dir at baseline — they enter v4 in Case C and will earn one before
their stone-ckpt closes. The "first bench" column below is the lead
row in each stone's bench output (often, though not always, the
hottest path); the full per-stone bench list lives in the JSON dump.

| stone | benches | first bench | median |
|---|---:|---|---|
| `mailrs-arc` | 4 | parse/aar | 27.062 ns |
| `mailrs-arf` | 2 | parse/hotmail_fbl_sample | 1.3305 µs |
| `mailrs-attachment-extract` | 2 | extraction_method/text_plain | 18.891 ns |
| `mailrs-auth-guard` | 6 | check/empty_map_success_path | 44.877 ns |
| `mailrs-backoff` | 12 | base_delay/attempt_3 | 1.7064 ns |
| `mailrs-clamav` | 6 | parse_response/clean | 9.9098 ns |
| `mailrs-clean` | 8 | clean_email_html/short_60b | 10.514 µs |
| `mailrs-dav` | 21 | etag_of | 53.299 ns |
| `mailrs-delivery-executor` | 1 | DeliveryExecutor::spawn | 554.52 ns |
| `mailrs-dkim` | 9 | parse/minimal/mailrs_dkim | 143.12 ns |
| `mailrs-dmarc` | 4 | generate_xml/n10 | 12.794 µs |
| `mailrs-dnsbl` | 6 | reverse_ipv4 | 46.139 ns |
| `mailrs-ical` | 11 | parse/simple_vevent/mailrs_ical | 1.5513 µs |
| `mailrs-imap-codec` | 1 | ImapCodec::decode/LOGIN | 70.059 ns |
| `mailrs-imap-format` | 3 | format_imap_flags/seen+answered | 19.738 ns |
| `mailrs-imap-proto` | 17 | parse/select/mailrs_imap_proto | 57.608 ns |
| `mailrs-inbound` | 11 | decision/make_delivery_decision_accept | 33.740 ns |
| `mailrs-intelligence` | 3 | extract_structured_data/short_single_event | 687.11 ns |
| `mailrs-jmap` | 23 | dispatch_mailbox_get | 3.6408 µs |
| `mailrs-mailbox` | 15 | insert_message/first_insert | 288.09 ns |
| `mailrs-maildir` | 13 | deliver_loop/n=1 | 4.6378 ms |
| `mailrs-mime` | 9 | parse/simple_text_plain | 46.207 ns |
| `mailrs-mta-sts` | 8 | parse/sts_record | 75.903 ns |
| `mailrs-outbound-queue` | 4 | dkim_sign/short | 287.73 µs |
| `mailrs-postmaster` | 1 | extract_bimi_logo_url | 39.775 ns |
| `mailrs-rate-limit` | 11 | hot_allowed/mailrs_rate_limit | 12.651 ns |
| `mailrs-rfc2047` | 11 | decode/ascii_passthrough | 21.539 ns |
| `mailrs-rfc2231` | 7 | encode/ascii_legacy_quoted | 23.428 ns |
| `mailrs-rfc5322` | 17 | header_lookup_subject_and_from/mailrs_rfc5322/1 | 221.92 ns |
| `mailrs-shield` | 7 | dnsbl/reverse_ipv4 | 47.342 ns |
| `mailrs-sieve` | 2 | compile_sieve/typical | 1.1839 µs |
| `mailrs-smtp-client` | 9 | parse_response/short | 23.605 ns |
| `mailrs-smtp-codec` | 2 | has_smuggle_sequence/safe | 2.5840 ns |
| `mailrs-smtp-proto` | 16 | parse/ehlo/mailrs_smtp_proto | 6.6309 ns |
| `mailrs-spf` | 9 | parse/simple/mailrs_spf | 45.819 ns |
| `mailrs-srs` | 4 | rewrite/ascii_sender | 190.37 ns |
| `mailrs-tls-rpt` | 5 | parse/record_single | 194.09 ns |
| `mailrs-webhook-signature` | 9 | sign/short_payload_23_bytes | 260.56 ns |

**Stones missing baseline (Case C — `benches/` absent)**:
`mailrs-acme`, `mailrs-dns`, `mailrs-tls-reload`, `mailrs-mail-builder`,
`mailrs-sieve-core`. These will receive their first bench at their
respective stone-ckpts.

**Reproduce**: `env -u TMPDIR cargo bench --workspace` from workspace
root. Total wall-clock ≈ 50 minutes on Mac16,11.

## Measured

### E2E inbound throughput with PG (the real bottleneck)

The `smtp_load` rows below intentionally skip PG persistence (per their
exclusion comment); this row uses `crates/server/tests/inbound_pg_throughput.rs`
to measure the **full inbound persistence path**:
`PgMailboxStore::append_message → maildir deliver + PG queries`. All
numbers below collected 2026-05-26 on dev PG 18, M-series Mac, local
NVMe, single shared dev cluster. The cluster is a **shared dev box**:
absolute numbers vary across runs depending on what else is sharing
the WAL/checkpoint queue, so the table below distinguishes (a) the
**best-case observation** under an unusually quiet PG and (b) the
**apples-to-apples comparison** between candidate code patterns
benchmarked back-to-back on the same PG state.

#### Best-case observed (quiet PG, sync_commit=on)

| Scenario | msg/s | p50 | p95 | p99 | Notes |
|---|---:|---:|---:|---:|---|
| **Round 29 baseline** (tx-wrapped SELECT FOR UPDATE → INSERT → UPDATE) | | | | | |
| 1 mailbox, 4 workers | 16.6 | 80 ms | 773 ms | 3.2 s | FOR UPDATE row lock serialises deliveries |
| 100 mailboxes, 8 workers | 50.3 | 79 ms | 423 ms | 2.1 s | PG WAL fsync is the floor |
| 100 mailboxes, 8 workers, `synchronous_commit=off` | 128.1 | 53 ms | 130 ms | 285 ms | Fsync removed — proves PG WAL was the floor |
| **Round 30 (autocommit, atomic UPDATE-RETURNING + INSERT)** | | | | | |
| 1 mailbox, 4 workers | **134.3** | 27 ms | 55 ms | **69 ms** | best of several runs; **+8.1× thru, −98% p99** |
| 100 mailboxes, 8 workers (msgs=2000) | **235.8** | 33 ms | 52 ms | **60 ms** | best of several runs; **+4.7× thru, −97% p99** |

#### v5 baseline (post R30-R38, 2026-05-26 quiet PG window)

Re-measured after the round 30-38 wave (FOR UPDATE drop, async tokio::fs
for all read paths, ASCII-fold sweep, SKIP LOCKED claim). 3 runs each,
sync_commit=on, same dev-PG cluster as R29:

| Scenario | msg/s (3-run median) | p50 | p95 | p99 | Notes |
|---|---:|---:|---:|---:|---|
| 1 mailbox, 4 workers | **169.2** | 24 ms | 38 ms | **48 ms** | cumulative 16.6 → 169 = **10.2×** vs R29 |
| 10 mailboxes, 4 workers | **220.5** | 18 ms | 27 ms | 34 ms | cumulative 35.0 → 220 = **6.3×** |
| 100 mailboxes, 8 workers (msgs=2000) | **238.7** | 33 ms | 50 ms | **60 ms** | cumulative 50.3 → 238 = **4.7×** |

These are the v5 phase-0 starting numbers. p99 fanout=1: 3.2s → 48ms = **−98.5%**.
v5 phases 1-7 will be measured against this row.

#### v5 R43: PG batch INSERT via `index_messages_batch`

Added `PgMailboxStore::index_messages_batch(&[IndexRecord])` — one
explicit tx, K UPDATE-RETURNING per unique mailbox to reserve uids,
one multi-row INSERT for all rows. Bench harness gained
`BASELINE_BATCH_SIZE` env var so workers can buffer K-message batches.

| Scenario / batch | b=1 | b=4 | b=16 | b=32 |
|---|---:|---:|---:|---:|
| fanout=1 (one mailbox) | 165 | **235** | 242 | 244 |
| fanout=100 (round-robin) | 197 | **258** | 60 ❌ | 24 ❌ |

Findings:

1. **batch ≤ 4 is an unconditional win** (single +43%, multi +31% vs b=1).
2. **batch ≥ 16 with mixed-mailbox traffic deadlocks**: a single tx
   holding 16 mailbox row locks contended with 8 concurrent workers
   produces PG deadlock aborts (msgs completed: 2000 → 768, p999
   spikes to ~400ms). Lock-order across batches isn't well-defined
   when each worker picks an arbitrary K-prefix of the message
   queue.
3. **Single-mailbox batches scale freely**: fanout=1 at batch=32
   still gains.

**Production implication for R44**: the upcoming DeliveryExecutor
at the PG layer **must buffer per-mailbox** (one batch = one
mailbox). Cross-mailbox batching causes lock-order conflicts that
no amount of retry escapes deterministically. Per-mailbox buffer
+ K up to ~32 is the right ceiling.

#### Apples-to-apples R30 vs R31-A tx (same PG state, back-to-back, round 31)

R31 directly compared the round-30 autocommit pattern against an
explicit `BEGIN; UPDATE; INSERT; COMMIT` tx variant (R31-A) by
truncating + re-running each three times on the same dev cluster.
Both variants were below their best-case ceiling because the cluster
had picked up unrelated WAL/checkpoint pressure between R30's original
measurement and R31's; but the **relative** ordering is the load-bearing
signal:

| Scenario | R30 autocommit | R31-A tx | R30 advantage |
|---|---:|---:|---|
| 1 mailbox, 4 workers (median of 3) | 18.8 msg/s, p99 2.7 s | 21.2 msg/s, p99 1.9 s | within noise; both stuck on WAL backlog |
| 100 mailboxes, 8 workers (msgs=2000) | **110.0 msg/s, p99 809 ms** | 74.2 msg/s, p99 1.36 s | **+48% throughput, −41% p99** ✅ |

The fanout=100 case is the decisive comparison — under any concurrency
the autocommit pattern wins. The fanout=1 case is dominated by WAL
backlog at this PG state and cannot rank the two patterns.

**Key finding**: at the e2e layer, neither sqlx itself nor the
`test_before_acquire(true)` setting nor any Rust-side allocation we
chased in rounds 16-28 is the bottleneck. The real ladder is:

1. **`FOR UPDATE` on `mailboxes` row** (the pre-round-30 pattern)
   — caps single-mailbox throughput at ~17 msg/s regardless of worker
   count or pool size. Distribute across N mailboxes ⇒ throughput
   scales linearly until step 2 kicks in. Round 30 removes this floor
   by collapsing `SELECT FOR UPDATE` + `UPDATE` into a single
   `UPDATE … RETURNING`.
2. **Per-tx WAL fsync (group-commit defeat)** — wrapping the two
   write statements in an explicit `BEGIN; … COMMIT;` forces one
   fsync per delivery. Round 31 measured this directly: explicit-tx
   regresses 1.5× on fanout=100 (110 → 74 msg/s) vs autocommit, even
   under the same PG state and schema, because PG's group-commit
   (`commit_delay` / `commit_siblings`) can only coalesce concurrent
   autocommit COMMITs into shared fsyncs at the WAL layer; explicit
   per-tx COMMITs each force their own fsync, defeating the batch.
   Counter-intuitive vs "1 fsync per tx beats 2 autocommit fsyncs"
   reasoning; reproducible — autocommit kept.
3. **Disk write throughput** — at `synchronous_commit=off` we hit
   ~128 msg/s; the floor here is maildir fsync + PG WAL writeback,
   neither of which sqlx or the Rust side can move.

**Caveat on absolute numbers.** The "best-case observed" rows above
(134.3 / 235.8) were the highest reproducible reading at the time
the change landed; they are not a stable ceiling — re-running on the
same dev cluster a few hours later (with different WAL/checkpoint
state) yielded 18-30 / 110 msg/s for the same code. The
*architectural* round-30 win (FOR UPDATE removal) and round-31
finding (autocommit > tx) survive in every re-measurement and are the
load-bearing claims; the specific msg/s numbers should be treated as
"observed once, easily affected by what else lives on the PG host".

#### v5 phase 0-4 ceilings (R39 → R51, 2026-05-26)

The v5 wave pushes each layer separately:

| Round | Layer | Change | Measured impact |
|---|---|---|---|
| R39 | PG | re-measure post R30-R38 baseline | 169 / 220 / 238 msg/s (fanout 1 / 10 / 100); p99 48 ms (single mailbox) |
| R43 | PG | `index_messages_batch` atomic K-msg INSERT (mailbox 1.1, `BASELINE_BATCH_SIZE` knob added) | b=4: fanout=1 235 (+39%), fanout=100 258 (+8%). b≥16 deadlocks on multi-mailbox (lock-order). Per-mailbox buffering required for prod wire-up. |
| R44 | PG | tried single-statement CTE; **negative** finding | -6% on fanout=100, PG CTE materialization > 1-RTT save. Reverted. |
| R47 | Disk | maildir `sync_all` → `sync_data` (fdatasync) | -1 journal write per delivery on Linux; macOS no-op |
| R48 | Disk | delivery-executor: `std::thread::scope` per-path parallel flush | Multi-recipient bursts: N×fsync no longer serial |
| R50 | Outbound | DKIM sign + ARC seal `buffer_unordered(8)` | Sequential signing → 8-way parallel; CPU-bound RSA on blocking pool |
| R51 | DNS | hickory cache 32 → 4096 | SPF/DKIM/DMARC repeats stay in cache |

**Cumulative since R29 baseline (single dev cluster, same hardware):**

| Path | R29 | R39 (v5 phase 0) | Cumulative |
|---|---:|---:|---|
| fanout=1, 4w (msg/s) | 16.6 | **169** | **10.2×** |
| fanout=10, 4w (msg/s) | 35.0 | **220** | **6.3×** |
| fanout=100, 8w (msg/s) | 50.3 | **238** | **4.7×** |
| fanout=1 p99 | 3.2 s | **48 ms** | **−98.5%** |
| fanout=100 p99 | 2.1 s | **60 ms** | **−97.1%** |

R43 batch API adds another +30-40% on top when callers can buffer
(b=4 across mailboxes), but production wire-up requires a
per-mailbox accumulator — not yet built (R52 candidate).

#### Negative findings recorded so future ops don't re-litigate

* **CTE INSERT (R44)** regresses 6% on fanout=100 vs the 2-stmt form
  because PG materialises CTE results between UPDATE and INSERT.
* **Explicit BEGIN/COMMIT around the two writes (R31)** regresses
  1.5× on fanout=100 — PG's group-commit can only batch
  *autocommit* COMMITs, not explicit tx ones.
* **`mail-auth` `default-features = false, features = ["ring"]`** to
  eliminate aws-lc-sys was tried and reverted: aws-lc-sys is still
  pulled by `instant-acme` / `jsonwebtoken` / `rcgen`, so the swap
  links BOTH crypto libs and INCREASES binary size. Eliminating
  aws-lc-sys requires switching all four upstream consumers; not
  in scope here.

Reproduce:
```bash
docker exec dev-postgres psql -U postgres -c "CREATE DATABASE mailrs OWNER mailrs"
docker exec -i dev-postgres psql -U mailrs -d mailrs < scripts/init-schema.sql
for f in scripts/migrate-*.sql; do
  docker exec -i dev-postgres psql -U mailrs -d mailrs < "$f"
done
MAILRS_PG_URL='postgres://mailrs:mailrs@127.0.0.1:5432/mailrs' \
  BASELINE_MSGS=500 BASELINE_WORKERS=4 BASELINE_MAILBOX_FANOUT=1 \
  cargo test -p mailrs-server --test inbound_pg_throughput --release \
  -- --ignored --nocapture | grep BASELINE_RESULT
```

The 28 crate-level optimization rounds (16-28) sit upstream of these
PG-anchored bottlenecks: their CPU savings are real and load-bearing
once the e2e path is freed of the lock + fsync floors. The next
e2e perf wave (planned task list) targets the row-lock + fsync ladder
directly:

* **Drop `FOR UPDATE` on mailboxes — use `nextval()` sequence per
  mailbox.** Estimated single-mailbox throughput 16.6 → ~50 msg/s
  (becomes fsync-bound, same as multi-mailbox).
* **Batch PG INSERTs (multi-row `INSERT … VALUES (...), (...)`).**
  Estimated 50 → ~250 msg/s at batch=32 (mirror of the
  `mailrs-delivery-executor` group-commit win).
* **Switch outbound-queue workers to `SELECT … FOR UPDATE SKIP
  LOCKED LIMIT N`.** Already a known PG pattern; saves multi-worker
  outbound from per-job lock contention.

### Workspace-level

| Path | Measurement | Run command |
|---|---|---|
| Release binary size (mailrs-server) | 44 MB (default) → 22 MB (perf-first profile). M-series Mac. | `du -h $TARGET_DIR/release/mailrs-server` before/after commit `9f21e0b`. |
| SMTP receive throughput (perf-first vs vanilla profile, original measurement 2026-05) | **+2.10%** throughput (267.2 vs 261.7 msg/s median, 3 rounds × 30s × 32 conns); **p99 latency −5.57%** (179.7 ms vs 190.3 ms). The original commit claim of "+10-20% throughput" was wrong; the real measured win is much smaller but still positive and consistent. Binary-size win is the dominant payoff of the perf-first profile. | `scripts/bench-smtp-load.sh 30 32 3` (builds both `release` and `release-vanilla` profiles, runs 3 rounds each, prints comparison) |
| SMTP receive throughput, **current** (post tracing + listener refactor, 2026-05-23) | **300.2 msg/s** (1 round × 30s × 32 conns, perf-first profile), **P50 106 ms, P99 152 ms, P999 166 ms** — single-round number, not a perf-first-vs-vanilla comparison. Logged here as the latest end-to-end number after all crate-level optimizations + the server-level listener helper refactor + tracing span addition. | `cargo bench -p mailrs-server --bench smtp_load --release -- --duration 30 --conns 32` |
| SMTP receive throughput, **post DeliveryExecutor** (`mailrs-delivery-executor` 1.0 group-commit, 2026-05-24) | **999 msg/s mean across 3 × 30s × 32 conns** (rounds: 1045 / 972 / 979). **3.4×** vs the immediately-prior 291 msg/s baseline (same hardware, same bench). **P50 32 ms** (vs 105 ms baseline = **3.3× faster**), **P99 41 ms** (vs 163 ms = **4.0× faster**), **P999 76 ms** (vs 199 ms = **2.6× faster**). All four UX axes — throughput, p50, p99, p999 — improve simultaneously; no axis regresses. The win comes from group-commit: 32 concurrent SMTP sessions delivering to the same Maildir path now share a single fsync per batch (max_batch=64, max_wait=10ms) via `mailrs-delivery-executor`'s mpsc → `Maildir::deliver_batch` pipeline, instead of each session driving its own per-message fsync. | `cargo build --profile release-debug -p mailrs-server --bench smtp_load && $CARGO_TARGET_DIR/release-debug/deps/smtp_load-* --duration 30 --conns 32 --warmup 5` |
| SMTP receive throughput, **post pipelined DeliveryExecutor** (`mailrs-delivery-executor` 1.1, max_concurrent_flushes=2, 2026-05-24) | **1079 msg/s mean across 3 × 30s × 32 conns** (rounds: 1074 / 1073 / 1089). **+8%** vs the 1.0 serial-flush 999 msg/s. **P50 29 ms** (-9%), **P99 36 ms** (-12%), **P999 45 ms (-41%)** — tail latency is the headline win. Mechanism: while batch A's fsync is in flight on a `spawn_blocking` thread, batch B starts collecting concurrently; a `Semaphore`-bounded pipeline of 2 in-flight flushes hides disk-wait behind batch-collection latency without queuing unbounded fsyncs. Cumulative since the perf-axis kickoff (#127): **291 → 1079 msg/s = 3.71× throughput**, **P999 199 → 45 ms = 4.4× faster tail**. | Same reproduce command as the 1.0 row above; binary uses the new published `mailrs-delivery-executor` 1.1 default tuning. |

### `mailrs-inbound` (criterion bench, M-series Mac, release, 100-sample median ± 95% CI from criterion's own analysis)

| Path | Median | Notes |
|---|---:|---|
| `decision::make_delivery_decision_greylist` | **2.4 ns** | trivial early return |
| `auth_header::build_auth_header_no_reason` | **30 ns** | was 342 ns; v4 round 7 direct String builder bypasses the `Vec<AuthResult>` + `format!` chain; **−91%** / 11× ✅ |
| `auth_header::build_auth_header_with_reason` | **34 ns** | was 429 ns; same change; **−92%** / 13× ✅ |
| `decision::make_delivery_decision_accept` | **30 ns** | was 337 ns; cascades the auth_header win; **−91%** / 11× ✅ |
| `decision::make_delivery_decision_dmarc_reject` | **46 ns** | was 408 ns; same auth_header cascade |
| `context::receive_context_to_pipeline_input` | **65 ns** | per-message snapshot clone |
| `pipeline_run/early_reject_short_circuit` | **201 ns** | first stage rejects → entire pipeline |
| `auth_header::format_auth_results_header_quadruple` | **197 ns** | RFC 8601 4-method header (generic Vec<AuthResult> path — still used by `Pipeline::run`; `build_auth_header` is the fast inbound-dispatch shortcut) |
| `decision::make_delivery_decision_junk` | **368 ns** | was 671 ns; cascades auth_header win + the build_junk_reason squeeze from commit `b8ea44d` |
| `pipeline_run/4_noop_stages` | **610 ns** | framework dispatch cost only |
| `pipeline_run/realistic_mix_6_stages` | **648 ns** | dispatch + 6 cheap noop-style stages |

Run: `cargo bench -p mailrs-inbound --bench pipeline` (the bench file
ships in `crates/inbound/benches/pipeline.rs`).

**v4 ckpt 15** (2026-06-03): Case A verified — `grep iter().position` /
`.windows(N)` / `push_str(&format!(...))` / `String::replace` in
src/ → 0 hits. The crate is 1.6k LOC across 6 files (decision /
auth_header / pipeline / stage / context / lib) with no string
parsing surface. Numbers re-confirmed against 3-run baseline;
matched within ±5 %. The big v4-round-7 win (build_auth_header
11× / build_junk_reason 2.4×) is durable and still load-bearing.

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

### Cross-ecosystem competitor map (C / C++ / Go / Python / Zig)

Per-crate competitor audit across 5 ecosystems (Rust competitors are
already covered in the head-to-head tables below). All entries verified
2026-05-26 via GitHub / PyPI / pkg.go.dev / zigistry.dev. This snapshot
covers the 41 crates published as of 2026-05-25; `mailrs-mail-builder`
and `mailrs-sieve-core` were added afterward and are not yet
cross-language audited. "—" means
no widely-used library found; "(monolith)" means the functionality
exists only inside a full MTA/server, not as a consumable library.
Verbose URLs intentionally elided here — full source list in
[v4 round 18 commit message].

#### Protocol parsers (12 crates)

| crate | C | C++ | Go | Python | Zig |
|---|---|---|---|---|---|
| smtp-proto | libetpan; Postfix/Sendmail (monolith) | vmime / Poco / mailio | emersion/go-smtp | aiosmtpd (server) / smtplib (client) | — |
| smtp-codec | (folded into proto) | (folded) | (bundled in go-smtp) | — | — |
| imap-proto | libetpan; Cyrus/Dovecot (monolith) | KDE KIMAP; vmime | emersion/go-imap; mjl-/mox/imap | imaplib / IMAPClient | — |
| imap-codec | (folded) | (folded into KIMAP) | (bundled) | — | — |
| imap-format | (folded) | (folded) | (bundled) | — | — |
| rfc5322 | GMime; libetpan; libcamel | KDE KMime; vmime | emersion/go-message; enmime; net/mail (stdlib) | **stdlib `email`** (canonical, 25 yrs) | — |
| rfc2047 | GMime; libcamel | KMime | mime stdlib + go-message | stdlib `email.header` | — |
| rfc2231 | GMime; libcamel; libetpan | KMime | stdlib mime + go-message | stdlib `email.utils` | — |
| mime | GMime; libetpan; libcamel | KMime; vmime; Poco | emersion/go-message; enmime; stdlib multipart | stdlib `email.message` | — |
| ical | **libical** (canonical 2025) | KDE KCalendarCore (wraps libical) | emersion/go-ical | **icalendar** (canonical, 2026 active) | — |
| jmap | Cyrus (monolith) | Cyrus (C, no native C++) | foxcpp/go-jmap; rockorager/go-jmap | jmapc (niche) | — |
| dav | Cyrus (monolith) | **KDE KDAV / KDAV2** | emersion/go-webdav | caldav (client); Radicale (server) | mail-os/mail (inline) |
| sieve | Pigeonhole (Dovecot plugin) | KDE libksieve | foxcpp/go-sieve; emersion/go-sieve | sievelib | — |

#### Email authentication (8 crates)

| crate | C | C++ | Go | Python | Zig |
|---|---|---|---|---|---|
| spf | libspf2 (stale 2013) | — (C dominates) | mileusna/spf; mox/spf | pyspf (stale 2020) | mail-os (inline) |
| dkim | **OpenDKIM** (dormant since 2018 beta) | halon/libdkimpp (rare native C++) | emersion/go-msgauth; mox/dkim | **dkimpy** (DKIM+ARC+TLSRPT) | mail-os (inline) |
| dmarc | OpenDMARC (2024) | — | go-msgauth; mox/dmarc; maddy | checkdmarc + parsedmarc | mail-os (inline) |
| srs | libsrs2 (stale 2018); postsrsd (live) | — | mileusna/srs (stale) | pysrs/srslib | — (totally absent) |
| arc | OpenARC (2024) | — | mox/dkim only (no standalone) | **dkimpy** (bundled) | mail-os (inline) |
| arf | — | halon-extras/arf (Halon plugin) | — | parsedmarc (partial) | — |
| tls-rpt | sys4/libtlsrpt | halon-extras (mostly node) | mox/tlsrpt | dkimpy (sign); parsedmarc (ingest) | mail-os (inline) |
| mta-sts | Snawoot/postfix-mta-sts-resolver (Python) | halon-extras | emersion/go-mta-sts (stale); mox/mtasts | postfix-mta-sts-resolver | mail-os (inline) |

#### Infrastructure primitives (9 crates)

| crate | C | C++ | Go | Python | Zig |
|---|---|---|---|---|---|
| dnsbl | — (3-line DNS, everyone rolls own) | — | godnsbl (small) | — (use dnspython directly) | mail-os (inline) |
| rate-limit | Postfix anvil (monolith) | **Facebook folly TokenBucket** | **golang.org/x/time/rate** (stdlib-ish) | **limits** (Redis/Memcached backed) | **minhqdao/zimit** (GCRA) |
| auth-guard | fail2ban (Python); Postfix postscreen (monolith) | — | — (rolled in-house) | — (FastAPI middleware) | — |
| clamav | libclamav (engine, not client) | libclamav (C, called from C++) | dutchcoders/go-clamd | python-clamd / clamav-client | — |
| backoff | — | kingsamchen/backoffxx (header-only) | **cenkalti/backoff/v5** (canonical) | `backoff`; `tenacity` | — |
| webhook-signature | OpenSSL HMAC (primitive) | OpenSSL HMAC | standard-webhooks; svix | pyca/cryptography (primitive) | std.crypto.HmacSha256 (primitive) |
| tls-reload | (SIGHUP reload in nginx/Postfix) | (manual SSL_CTX swap) | (stdlib GetCertificate + in-mem swap) | pyOpenSSL context replace | — (no rustls in Zig; BearSSL/OpenSSL bindings only) |
| acme | **uacme**; OpenBSD acme-client | jmccl/acme-lw | **certmagic; lego; acmez; autocert** (4 mature) | **certbot/acme** (the reference impl) | mail-os (inline) |
| dns | **c-ares** (curl/Node); ldns; getdns | c-ares | **miekg/dns** (universal) | **dnspython** (canonical) | lun-4/zigdig (44⭐ "naive"); zig-dns (66⭐ stale) |

#### Server building blocks (12 crates)

| crate | C | C++ | Go | Python | Zig |
|---|---|---|---|---|---|
| smtp-client | libESMTP; libetpan | vmime/Poco/mailio | emersion/go-smtp; mox/smtpclient | smtplib / aiosmtplib | karlseguin/smtp_client.zig (TLS hole) |
| outbound-queue | Postfix qmgr (monolith) | — | mox/queue; maddy/queue | Salmon; Mailman 3 | — |
| maildir | libetpan; Dovecot/Courier (monolith) | KDE Akonadi resource | emersion/go-maildir | stdlib `mailbox.Maildir` | — |
| mailbox | Dovecot lib-storage (monolith) | KDE Akonadi | mox/store; maddy/storage | stdlib + Modoboa/Mailman | — |
| inbound | **libmilter** (closest analogue) | — | **maddy/msgpipeline** (closest mirror) | **Salmon** | mail-os (monolith) |
| shield | postgrey (Perl); rspamd (monolith C) | rspamd | maddy/check + mox/junk (bayesian) | — (SpamAssassin is Perl) | — |
| postmaster | — (checkdmarc / internet.nl as services) | — | mox check (CLI) | — (bespoke) | — |
| intelligence | — (LLM-era, no precedent) | — | — | — | — |
| clean | libtidy (partial overlap) | gumbo-parser; KDE messagelib sanitizer | **bluemonday** (canonical Go) | **nh3** (Rust-backed via PyO3) | — |
| delivery-executor | Postfix/Dovecot deliver (monolith) | Dovecot LDA | mox; maddy/target | Mailman 3 outgoing runner | — |
| attachment-extract | poppler + Tesseract (shell-piped) | KMime + libpoppler-cpp | ledongthuc/pdf + gosseract | PyPDF2/pypdf + pytesseract | — |

#### Where each ecosystem stacks up

**Coverage by ecosystem (out of 41 crates, intelligence excluded — 40 measurable):**

| Ecosystem | Direct crate-level competitor | Monolithic-only (no carve-out) | No competitor at all |
|---|---:|---:|---:|
| **C** | ~22 (parsers + auth + several infra) | ~14 (Postfix/Cyrus/Dovecot/Sendmail internals) | ~4 (intelligence, tls-reload, several niches) |
| **C++** | ~15 (KDE PIM dominates parser/storage) | ~10 (Cyrus/rspamd) | ~15 (huge auth + infra gap) |
| **Go** | ~28 (Maddy + Mox + emersion + mileusna + acme cluster) | ~6 (mox/maddy internals) | ~6 (arf, arc-standalone, auth-guard, postmaster, etc.) |
| **Python** | ~26 (stdlib email + dkimpy + Salmon + Mailman + certbot + nh3) | ~3 | ~11 (smtp/imap proto crates, JMAP server, anti-spam native) |
| **Zig** | **3** (zimit rate-limit, zigdig DNS, karlseguin/smtp_client) | ~18 (all bundled in 6⭐ mail-os/mail monorepo) | ~20 (totally absent) |
| **Rust (us)** | 41 (full federated split) | 0 | 0 |

**Key qualitative findings:**

1. **The C email-auth stack is dormant.** OpenDKIM hasn't cut a release
   since 2018 beta; libspf2 since 2013; OpenDMARC since 2023. mailrs's
   `dkim`/`spf`/`dmarc`/`arc` crates fill a real abandonment gap that
   the entire C ecosystem has not addressed in 5-12 years.
2. **Go is the closest peer.** `Maddy` (foxcpp) + `Mox` (mjl-) are the
   two Go mail servers with similar architectural ambition; emersion's
   GitHub org is the canonical pure-protocol-parser maintainer.
   Coverage is dense (~28 of 40) but most of Maddy's packages are
   `internal/` and therefore not re-usable as libraries — mailrs's
   crate-federation model is structurally different.
3. **Python wins on legacy depth.** stdlib `email` covers 4 crates in
   one 25-year-old package; `icalendar` and `certbot/acme` are the
   reference implementations for the world. But everything is
   ≥20× slower than the Rust equivalents by GIL/interpreter overhead
   — comparison is structural, not unfair.
4. **C++ email ecosystem ≈ KDE PIM.** KMime / KIMAP / KCalendarCore /
   KDAV / libksieve cover most parser+storage crates. Outside KDE, only
   vmime + Poco + mailio survive as full-featured email clients. Email
   auth in C++ is essentially absent (lone exception: halon/libdkimpp).
5. **Zig is years behind.** Three real standalone crates exist (zimit,
   zigdig, karlseguin/smtp_client). One 6-star monorepo (mail-os/mail,
   alpha) bundles ~18 inline; 20 crates have **no Zig implementation
   anywhere**. SRS, ARF, JMAP, Maildir, RFC 5322 are completely
   untouched by Zig.
6. **mailrs's per-RFC crate-granularity has no direct analogue in
   any ecosystem.** C/C++ ship monolithic MTAs or huge frameworks (KDE
   PIM); Go bundles into Maddy/Mox; Python has the stdlib `email` mega-
   module + DKIM/ARC/TLSRPT-bundled `dkimpy`. Only the Rust ecosystem
   (and only mailrs, plus stalwart) ship one published crate per RFC.

Sources verified by 5 parallel research agents 2026-05-26 against
GitHub, PyPI, pkg.go.dev, zigistry.dev, and project websites. Full
URL list lives in the v4-round-18 commit body.

### Crate size — release `.rlib` per published crate

41 published crates, sorted by release-mode `.rlib` size
(`cargo build --workspace --release` → top-level `target/release/lib*.rlib`,
which excludes upstream deps unlike `target/release/deps/`).

| Bucket | Crates | Range |
|---|---|---:|
| **Tiny** (≤50 KB, 9 crates) | imap_codec, rfc2231, srs, backoff, webhook_signature, rfc2047, smtp_codec, sieve, rfc5322 | 20–39 KB |
| **Small** (50–110 KB, 11 crates) | arf, attachment_extract, auth_guard, clamav, shield, maildir, rate_limit, tls_reload, mime (97), delivery_executor, imap_format | 56–108 KB |
| **Medium** (110–500 KB, 10 crates) | mta_sts, dnsbl, inbound, imap_proto, smtp_proto, postmaster, arc, ical, dav, clean | 117–496 KB |
| **Large** (≥500 KB, 11 crates) | smtp_client (563), jmap (591), tls_rpt (678), dns (779), spf (930), dkim (1008), intelligence (1014), acme (1163), dmarc (1432), outbound_queue (1579), mailbox (1659) | 563–1659 KB |

Note: `mime` was 143 KB before the v4 round 13 single-pass header collect
landed — the refactor removed 5 distinct `Message::header()` / `Message::body()`
call sites per `Part`, so monomorphisation shrinks too. **97 KB now (–32 %)**.

The "Large" bucket is dominated by crates with crypto + DNS + storage backends
(`dkim` has rsa/ed25519, `dmarc` has the full reporter, `outbound_queue` /
`mailbox` link sqlx + tokio). The "Tiny" bucket is the pure-parser core; their
size is dominated by criterion-target machinery, not their own logic.

Reproduce:
```bash
cargo build --workspace --release
find target/release -maxdepth 2 -name 'libmailrs_*.rlib' -not -path '*/deps/*' \
  | xargs -I{} sh -c 'printf "%6dKB  %s\n" "$(stat -f%z "$1" 2>/dev/null \
    || stat -c%s "$1")" $(basename "$1" .rlib)' _ {} | sort -rn
```

### Memory profile — `dhat-rs` heap probes

Two `examples/dhat_profile.rs` shims live in-tree (`mime` + `spf`) — they
swap the global allocator for `dhat::Alloc` and exercise the hot path
10k times so per-call averages fall out of the totals. Run with
`cargo run --example dhat_profile -p mailrs-<crate> --release` to
re-derive these numbers; `dhat-heap.json` is gitignored.

| Probe | Total | Per-call avg | Peak in-flight | Leaks |
|---|---:|---:|---:|---:|
| `mime::parse(INVITE) + find_by_content_type` × 10 000 | 15.23 MB / 140 000 blocks | **1 523 B / 14 allocs** | 1 510 B / 11 blocks | 0 |
| `spf::Record::parse({simple, complex_8, pathological_8})` × 10 000 ea | 20.81 MB / 190 000 blocks | **694 B / 6.3 allocs** (avg over 3 inputs) | 616 B / 9 blocks | 0 |

The `mime` per-call cost (1 523 B / 14 allocs after v2.0
CompactString) is the parse-tree weight after the round-17
refactor: `ContentType.{type_, subtype}` and `Disposition.kind`
all inline into their structs (≤24 bytes ⇒ no heap), so the only
allocs that remain are the `Cow::Owned` `body` for transfer-encoded
parts plus the small `ContentType.params` HashMap nodes. Pre-v2.0
this was 20 allocs/call (the 3 type_ + 3 subtype Strings of a 3-
part invite tree all hit the heap); v2.0 cut 6 of those 20.
Zero leaks across 140 000 allocations confirms the recursive Walker
+ Cow tree shape drops cleanly on teardown.

The `spf` per-call cost (694 B / 6.3 allocs) is mostly the
`Mechanism::*` Vec growth (4-slot pre-sized in `Vec::with_capacity(4)`)
plus the boxed include-domain Strings. The peak (616 B / 9 blocks) is
the largest single record (`pathological_8` with 8 include strings)
alive at one moment — under 1 KB per record.

These are the two most-exercised crates (`mime` runs on every
inbound message, `spf` runs on every accepted MAIL FROM). Across the
two there's room for further reduction (e.g. inlining small Strings via
`SmolStr` for `ContentType.type_` / `subtype` — a 2.0 break that would
drop a known 8 allocs/call on mime). Not done yet; documented here as
a future axis.

### Test coverage — `cargo llvm-cov --workspace`

Workspace total (line-coverage, `cargo llvm-cov --workspace --summary-only`):
**63.67 % region / 67.47 % function / 58.66 % line** (2026-05-26).

The headline number is dragged down by `mailrs-server`'s web/admin/OIDC/RSVP
handlers — those are framework-wiring code that
[`testing.md`](.claude/rules/common/testing.md) explicitly puts in the
**Skip** bucket ("glue code, framework wiring, dependency injection setup,
trivial getters/setters"). Published crates look very different — sampled
from the cov report:

| Crate | line cov |
|---|---:|
| webhook-signature | 99.7 % |
| smtp-client/response | 99.8 % |
| srs | 98.8 % |
| smtp-codec | 97.7 % |
| smtp-proto (parse + session) | 97.7–98.1 % |
| sieve | 94.8 % |
| spf/evaluator | 92.2 % |
| storage-maildir | 92.0 % |
| tls-reload | 97.4 % |
| tls-rpt/record | 96.1 % |
| spf/record | 85.1 % |

Crates land at 85–99 % line coverage; everything below 80 % is server-side
framework wiring. The workspace 80 % bar from `testing.md` is satisfied for
all 41 published crates individually, even though the workspace-wide rollup
sits at 58.66 % because of the server binary.

Reproduce: `cargo llvm-cov --workspace --tests --summary-only --ignore-run-fail`
(perf_gate tests fail under coverage instrumentation due to inflated
budgets; `--ignore-run-fail` lets the summary still print).

### Head-to-head vs. Rust community competitors (criterion, M-series Mac, release profile, `--quick` mode)

Honest comparison. Wins **and** losses. Bench source: `crates/<crate>/benches/compare_<competitor>.rs` (each crate's compare bench is reproducible in-tree).

#### `mailrs-spf` vs `mail-auth` 0.9 (SPF half — the DEPS_AUDIT #1 reason)

3-run noise-controlled median (M-series Mac, release, criterion
default 100 samples × 3 fresh invocations):

| Input | mailrs-spf | mail-auth | Winner |
|---|---:|---:|---|
| `v=spf1 ip4:203.0.113.0/24 -all` (simple) | **43 ns** | 53 ns | **mailrs +23%** ✅ |
| 8-mechanism complex | **240 ns** | 440 ns | **mailrs +45%** ✅ |
| 8-include pathological | **223 ns** | 583 ns | **mailrs +62%** ✅ |

**Honest re-bench, v4 round 12 → 13 (2026-05-26):** previously
claimed "tied within noise" for the simple case was *under-claim*
— controlled 3-run median shows mailrs +22%. The complex_8 claim
of "+34%" was also conservative; real median is +37%. Pathological
got a fresh both-sides quiet-CPU bench: +50% lead (vs the prior
carried-forward +43%).

**v4 round 20 (2026-05-26 — spf 2.0 CompactString)**: bumped
`mailrs-spf` to **2.0.0**; `Mechanism::{A, Mx, Include, Exists}`
`domain` fields move from `String` / `Option<String>` to
`CompactString` / `Option<CompactString>`. The pathological_8
record (8 `include:` mechanisms) saved 8 heap allocations per
parse — mailrs's absolute time dropped from 290 ns to **223 ns
(-23%)**, pushing the lead over mail-auth from **+50% → +62%**.
The complex_8 case (1 `a:`, 1 `mx:`, 2 `include:`) saved 4 allocs
and gained +37% → +45%. Simple has no domain mechanisms so the
+23% number is unchanged. API break is contained to the
`Mechanism::*` enum variants via `CompactString::Deref<Target=str>`
+ `PartialEq<&str>`.

v4 round 4 + v4.next together closed the gap on the simple case
from −25% baseline to clear-lead +23%. Three changes:

1. **Single-pass byte IPv4 parser.** `<Ipv4Addr as FromStr>` does
   general-purpose UTF-8 char iteration + error machinery. Replaced
   with a per-byte state machine: walk the input once, build each
   octet inline, reject any non-digit/non-dot byte. Same shape as
   mail-auth 0.9's `Ipv4Addr` parser.
2. **`split(' ')` over `split_whitespace()`.** RFC 7208 §4.5 mandates
   single SP between mechanisms; the UTF-8-aware whitespace detector
   adds ~5 ns per token for no gain.
3. **`Vec::with_capacity(4)` + `parse_octet` reused for the CIDR
   prefix.** Pre-sizes the mechanisms Vec to the common-case count;
   the unrolled octet parser also handles the `/24` suffix.

v4.next round (commit landed): `Record::parse` rewritten as a
single-pass byte iterator (`bytes.iter()` + memchr-driven token
extraction + inline modifier filter in the same forward walk), and
the `all` mechanism (every record's terminator) is now byte-prefix-
detected and constructed inline without the `parse_mechanism`
call. This closes the simple-record gap to within ±1 ns CPU noise.

Status: every SPF input shape now matches or beats mail-auth.

#### `mailrs-dkim` vs `mail-auth` 0.9 (DKIM-Signature header parse)

3-run noise-controlled median (M-series Mac, release; re-measured
v4 ckpt 8, 2026-06-03):

| Input | mailrs-dkim | mail-auth | Winner |
|---|---:|---:|---|
| minimal (7 tags) | **147 ns** | 175 ns | **mailrs +19%** ✅ |
| realistic (folded, 11 tags, 7 signed headers) | **448 ns** | 433 ns | **mail-auth +3%** (TIE) |

**v4 ckpt 8 retraction (2026-06-03)**: the v4 round 16 numbers
(`minimal 121 ns`, `realistic 374 ns`) were single-run quiet-CPU
outliers — 3-run honest re-measure shows mailrs ~20-22% slower
than that earlier claim on both shapes (mail-auth side stayed
within noise). Lead margin: minimal +31% → +19%; realistic +14%
→ essentially TIE (mail-auth marginally ahead, within noise).
Structural advantage (CompactString for d/s/i/q tags, byte-level
tag dispatch) is unchanged and still load-bearing; the absolute
121 ns / 374 ns numbers are not reproducible.

**v4 round 16 (2026-05-26 — DkimHeader 2.0 CompactString)**: bumped
`mailrs-dkim` to **2.0.0**; switched the four `d=` / `s=` / `i=` / `q=`
tag fields from `String` to `compact_str::CompactString` (inline
≤24 bytes — real-world domains and selectors almost always fit).
On minimal-shape DKIM (1 domain + 1 selector + default q), the
hot path drops from ~6 String allocations to ~2 (just `b=` and
`bh=` which transform via `strip_wsp`). v4 round 16 measured drop:

  Before (v1.5): 183 ns minimal / 480 ns realistic
  After  (v2.0): 121 ns minimal / 374 ns realistic (← single-run)
  ckpt 8 honest: 147 ns minimal / 448 ns realistic (3-run median)

#### `mailrs-dkim::headers` — memchr-anchored header walk (v4 ckpt 8)

The verify + sign hot path. `collect_signed_headers` runs once per
outbound DKIM-sign; `find_header_value{,_in_raw}` runs once per
verified `DKIM-Signature` and per `From:` lookup. Both are per-
message ops; the byte scans they used to do were the only non-
memchr scanners left in the crate.

| Path | Median | Notes |
|---|---:|---|
| `collect_signed_headers/10 headers` | **986 ns** | was 1.05 µs; v4 ckpt 8 memchr2 + memchr scan **−6%** |
| `collect_signed_headers/30 headers` | **2.11 µs** | was 2.35 µs; same **−10%** |
| `collect_signed_headers/60 headers` | **3.72 µs** | was 4.27 µs; same **−13%** (alloc-bound on output, scan win small) |
| `find_header_value/first hit (Return-Path)` | **15.7 ns** | was 20.3 ns; **−23% / 1.29×** |
| `find_header_value/mid hit (Content-Type)` | **75.9 ns** | was 117 ns; **−35% / 1.54×** — typical-shape lookup |
| `find_header_value/missing (full walk)` | **417 ns** | was 748 ns; **−44% / 1.79×** — pure scan, no alloc |

**v4 ckpt 8** (2026-06-03): replaced 5 byte-by-byte `while ... != b'\n'`
scans in `headers.rs` with `memchr::memchr` (for pure line-skip in
`find_header_value{,_in_raw}` + folded-continuation walk in
`collect_signed_headers`) and `memchr::memchr2(b':', b'\n', ...)`
(for the per-line colon-and-LF scan that also tracks the first
colon). The pure-scan paths (`find_header_value/missing`) get
the cleanest 1.79× win; `collect_signed_headers` was alloc-bound
on the output `String::from_utf8_lossy + .to_string()` per header
so the scan win there was partly absorbed (resolved in ckpt 8.1
below).

**v4 ckpt 8.1** (2026-06-03): three layered improvements after the
ckpt 8 memchr base shipped:

1. **canon.rs `relax_body` memchr-anchored chunked extend** —
   replaced the upfront `Vec<&[u8]>` line split + per-byte WSP
   collapse loop with a `memchr(\n)` line walk + `memchr2(' ',
   '\t')` next-WSP anchor + `extend_from_slice` for the clean
   runs between WSP anchors. Wins (3-run median):

   - `canon_body/relaxed` (~40 B input): 369 ns → **111 ns** (3.3×)
   - `canon_body/relaxed/1kb`: 3.10 µs → **1.20 µs** (2.6×)
   - `canon_body/relaxed/5kb`: 10.97 µs → **8.88 µs** (1.24×)
   - `canon_body/relaxed/50kb`: 87.4 µs → **61.7 µs** (1.42×)

   Big inputs are memcpy-bound (the `extend_from_slice` is the
   floor), small inputs were dominated by per-line `Vec` setup so
   they get the biggest relative win.

2. **`CachedDkimResolver<R>` — parsed-key cache** in
   `crates/dkim/src/resolver.rs`. Wraps any `DkimResolver` and
   caches the post-`extract_public_key` byte string per
   `(selector, domain)` with a 5-minute TTL and 512-entry
   capacity. Adds a default-impl `lookup_public_key()` method on
   the `DkimResolver` trait so the verifier hot path
   (`verifier::verify_one`) calls the resolver once for the
   already-extracted bytes; cached resolvers short-circuit. On a
   hit the per-message cost drops from `lookup_txt + base64
   decode + DER strip + (aws-lc-rs ASN.1 parse during verify)` to
   `Arc::clone`. Inbound traffic to mailrs is heavily skewed
   toward a handful of high-volume senders (Gmail, Microsoft,
   mailing-list forwarders) so the steady-state hit rate is
   expected to be very high — practical win is hot-path latency,
   not throughput peak. **No breaking change** — the default
   trait method preserves existing `DkimResolver` implementations.

3. **`collect_signed_headers_borrowed` — zero-alloc occurrence walk**.
   New `pub fn` that returns `Vec<(&str, Option<&str>)>` borrowing
   into `headers_raw` (values) and `names` (names). Internal
   callers (`sign::sign` + `verifier::verify_one`) switched to
   the borrowed variant. Old `pub fn collect_signed_headers` →
   `Vec<(String, Option<String>)>` is kept as a thin wrapper for
   downstream API stability — also benefits because the walk no
   longer per-occurrence String-clones. Wins (3-run median):

   | n_headers | owned (wrapper) | borrowed (hot path) | vs ckpt 8 owned |
   |---|---:|---:|---:|
   | 10 | 449 ns | **231 ns** | 4.3× vs 986 ns (ckpt 8) |
   | 30 | 779 ns | **561 ns** | 3.8× vs 2.11 µs |
   | 60 | 1.35 µs | **1.08 µs** | 3.4× vs 3.72 µs |

   Bench coverage: 11 ops → 17 ops (canon_body 3 sizes added;
   collect_signed_headers split owned vs borrowed × 3 sizes).

Combined ckpt 8 + 8.1 effect on the per-DKIM-sign hot path:
- header walk (`collect_signed_headers_borrowed/60`): 3.72 µs →
  **1.08 µs** (3.4×) — the alloc-bound floor from ckpt 8 was
  resolved by Phase C borrowed API
- body canon (`canon_body/relaxed/5kb`): 10.97 µs → **8.88 µs**
  (1.24×) — memcpy-bound at the floor
- public-key verify lookup (`CachedDkimResolver` hit): ~10-100 µs
  → `Arc::clone` — order-of-magnitude on hit rate, prod-realistic
  drop given the few-senders-dominate inbound shape

Caveat: the 2.0 break changes pub field types
(`String` → `CompactString`). Most call sites compile unchanged
because `CompactString: Deref<Target = str>` and `PartialEq<&str>`.
Server crate's `mailrs-dkim` dep is gated on `path = "../dkim",
version = "2"` until the 2.0.0 publish lands on crates.io.

**Previous v4 round 12 framing (now superseded):** the previously
claimed "mailrs 1.8×" on minimal was a single-run quiet-CPU lucky
outlier under the 1.x parser; round 12 corrected the 1.x number
to a conservative +11% median, and round 16's CompactString
refactor reclaims a real +31% structural lead.

v4 round 9 replaced the `h=` signed-headers parse:
  `raw_val.split(':').map(|s| s.trim().to_ascii_lowercase()).collect()`
which allocates one `String` per signed header name (5-7 per
realistic signature), with a single byte-level forward scan that
lowercases in-place into a reused `Vec<u8>` and pushes finished names
on `:`. Same pattern as `arc::ArcMessageSignature::parse`.

Before the perf-batch (commit `8eba06c` and later) we were 4.1× / 3.6× slower than mail-auth. Two changes closed the gap and then surpassed it:
1. Single-pass byte scanner replaces the HashMap + unfold pre-pass.
2. Byte-level dispatch (`match name.as_bytes() { b"v" => ..., b"a" => ... }`) + byte-iter `h=` parsing with `from_utf8_unchecked` (safe because only ASCII bytes pushed).

44 inline tests unchanged. Body+header canonicalization comparison still deferred (mail-auth streams into a `HashContext` and we return `Vec<u8>` — apples-to-pears).

#### `mailrs-mime` vs `mail-parser` (MIME body parse)

3-run noise-controlled median (criterion default 100-sample,
each run a fresh `cargo bench` invocation; CI bands rejected
when system load contaminates a single run). Re-measured in
v4 ckpt 4 (2026-06-02):

| Input | mailrs-mime | mail-parser | Winner |
|---|---:|---:|---|
| simple `text/plain` body_text | **86 ns** | 210 ns | **mailrs +59%** ✅ |
| find `text/calendar` part (apples-to-apples) | **619 ns** | 664 ns | **mailrs +7%** ✅ (narrower than previously claimed +15% — see ckpt 4 note) |

The find-calendar comparison is true apples-to-apples — both sides
parse the message and walk parts looking for the `text/calendar`
mime-type, returning the body's length. Bench source:
`crates/mime/benches/mime.rs::bench_vs_mail_parser_invite`.

**Honest re-bench, v4 round 13 (2026-05-26):** the previously
claimed "+6% mailrs win on find_calendar" was a single-run CPU-noise
outlier — controlled 3-run repeated measurement showed mailrs was
actually **~28% slower** than mail-parser. The same noise control
caught us *under-claiming* the simple body_text win (real ~+45%,
not +17%). Re-bench discipline now applied to every close-call.

**v4 round 17 (2026-05-26 — mime 2.0 CompactString)**: bumped
`mailrs-mime` to **2.0.0**; switched `ContentType.{type_, subtype}`
and `Disposition.kind` from `String` to `compact_str::CompactString`
(inline ≤24 bytes). All real MIME top-level types ("text", "multipart",
"application") and subtypes ("plain", "html", "calendar",
"alternative", "mixed", "report") fit inline → zero alloc on every
leaf parse for those fields. Added `lower_compact()` helper so
already-lowercase inputs (the overwhelming wire-format case) skip
the intermediate `String::to_ascii_lowercase` alloc entirely.

Measured:

  Before (1.0.4): simple 108 ns | find_calendar ~620 ns
  After  (2.0.0): simple  84 ns | find_calendar  539 ns
  Δ:             −22% simple   | −13% find_calendar

Lead over mail-parser:

  Before (1.0.4): simple +45% | find_calendar +5-10% (borderline)
  After  (2.0.0): simple +57% | find_calendar +15% (clean, out of noise)

dhat per-call alloc count: 20 → 14 (−30%, 6 heap allocs saved
on the 3-Part invite shape: 3 type_ + 3 subtype Strings replaced
by inline CompactStrings). Per-call bytes 1564 → 1523. Peak in-
flight 1551 / 17 blocks → 1510 / 11 blocks.

**Round 13 fix — single-pass header collection.** The dominant cost
in `parse()` was 5× redundant scans of the header region: 4×
`Message::header()` lookups (Content-Type, Content-Disposition,
Content-ID, Content-Transfer-Encoding) + 1× `Message::body()`,
each doing its own forward sweep. Replaced with one byte-walk that
dispatches each `Content-…:` line to its slot, captures the body
offset on the empty-line terminator, and exits. Plus inlined a
memchr-based unfold helper to skip past LF positions. Total work
dropped from `5 × O(H) per Part` to `1 × O(H) per Part`. On the
multipart-with-2-leaves invite shape that's 9 fewer header sweeps
per parse — `find_calendar` mailrs side moved from ~1050 ns to
~620 ns, reversing the −28% loss to a +5-10% lead.

v4.next landed: `Part` is now lifetime-parameterized (`Part<'a>`)
and `body` switched from `Vec<u8>` to `Cow<'a, [u8]>`.
`TransferEncoding::decode` returns `Cow::Borrowed(input)` for the
identity encodings (7bit/8bit/binary/Other — the common case),
zero allocation for leaf bodies. **Breaking API change** for direct
consumers: the field now needs `&*part.body` or `part.body.as_ref()`
to coerce to `&[u8]`. mailrs-server + mailrs-arf updated; downstream
consumers will need to add the same deref.

Prior rounds (still load-bearing): memchr-based boundary scan in
`split_multipart`, `Vec::with_capacity(4)` for parts, slice-only
boundary comparison (no per-call delimiter Vec build).

**v4 round 24 (2026-05-26 — mime 2.0.1 base64 fast-path)**: the
old `decode_base64` always allocated an intermediate `cleaned: Vec<u8>`
to strip WSP before feeding base64. For payloads with no whitespace
(short single-line signatures, inline images packed without 76-col
wrapping) this was pure waste — the entire encoded payload got
copied byte-for-byte just to confirm there was nothing to remove.
v2.0.1 probes WSP with memchr (SIMD-vectorised), and feeds the
original slice straight to base64 when clean.

Honest 3-run medians re-measured in v4 ckpt 4 (2026-06-02):

  decode_base64/clean_4k:    ~2.5 µs   (no WSP — fast-path)
  decode_base64/wrapped_4k:  ~6.5 µs   (RFC 2045 76-col WSP — strip path)

The original v4 round 24 entry claimed `clean_4k: 1.43 µs` and a
"4.2× faster than wrapped" ratio. That was a single-run measurement
taken on a quiet system; re-measured across 3 fresh runs on the same
hardware, clean_4k holds at ~2.5 µs (1.74× faster than wrapped, not
4.2×). The structural win is real — the fast-path skips a full
copy of the encoded body — but the over-claim is retracted. The
fast-path still eliminates the per-byte strip cost; what was wrong
was the absolute number.

**v4 ckpt 4 (2026-06-02): mailrs-mime Case A verified.**
- No exploitable hot path beyond what v4 rounds 13 / 17 / 24 already
  shipped (single-pass header walk, CompactString for type/subtype,
  Cow<[u8]> for body, base64 WSP fast-path).
- `grep iter().position` / `windows(N)` in src/ → 0 hits. The 4 hot
  paths all use `memchr` (boundary scan, line scan, header walk,
  base64 WSP probe).
- Numbers re-measured 3-run honest above. find_calendar lead
  narrowed slightly (+15% → +7%) — likely because the v4 round 17
  CompactString gain on `find_calendar` was measured against a
  particular mail-parser version state and small re-build variance
  has crept in. Still net win on every measured shape.

#### `mailrs-rfc5322` vs `mail-parser` (header lookup, lazy)

mailrs-rfc5322 is pull-based: it scans for the requested header without parsing the body. mail-parser eagerly parses everything. Comparison is therefore by body size — the lazy crate's wall-clock cost is constant.

| Body size | mailrs-rfc5322 (subject + from) | mail-parser (full parse) | Winner |
|---|---:|---:|---|
| 1 KB | **83 ns** | 2.63 µs | **mailrs 32×** ✅ |
| 5 KB | **84 ns** | 3.73 µs | **mailrs 44×** ✅ |
| 20 KB | **84 ns** | 7.68 µs | **mailrs 91×** ✅ |

This is the "lazy beats eager" payoff under load. If you only need 1-2 headers per message — which the SMTP frontline does — `mailrs-rfc5322` is the right tool. Use `mail-parser` when you need full-tree access in one shot.

**v4 round 1** (2026-06-02) tripled the speedup ratio above by
swapping two `iter().position()` byte-scans for `memchr::memchr` in
the header scanner — see the detailed table below for the per-op
breakdown.

#### `mailrs-rfc2047` vs `mail-parser` (subject extraction)

| Input | mailrs-rfc2047 (single-field) | mail-parser (full message) | Winner |
|---|---:|---:|---|
| ASCII subject | 23 ns | 323 ns | **mailrs 14×** ✅ |
| =?UTF-8?B?...?= encoded | 85 ns | 346 ns | **mailrs 4×** ✅ |

Same caveat as rfc5322: the right comparison is "minimum cost to get the user-visible Subject string", and a focused crate beats a tree builder. mail-parser remains the right call when you want the full structured Message at once.

#### `mailrs-ical` vs `icalendar` 0.17 (RFC 5545 parse)

3-run noise-controlled median:

| Input | mailrs-ical | icalendar | Winner |
|---|---:|---:|---|
| simple VEVENT | **1.37 µs** | 6.07 µs | **mailrs 4.4×** ✅ |
| VEVENT + RRULE | **1.60 µs** | 6.63 µs | **mailrs 4.1×** ✅ |
| VTIMEZONE + VEVENT | **2.73 µs** | 10.70 µs | **mailrs 3.9×** ✅ |

**v4 round 21 (2026-05-26 — ical 2.0 CompactString)**: bumped
`mailrs-ical` to **2.0.0**; three high-frequency String fields move
to `compact_str::CompactString`:
  * `RawComponent.name` — `VEVENT` / `VALARM` / `STANDARD` etc (6-10 B)
  * `RawProperty.name` — `DTSTART` / `SUMMARY` / `ATTENDEE` etc (5-10 B)
  * `VTimezone.tzid` — `America/New_York` / `Asia/Tokyo` etc (10-20 B)

All real-world iCal component/property names fit the 24-byte inline
buffer, saving one heap alloc per name. A VEVENT with 10 properties
saves ~11 String allocs per parse.

Measured (3-run quiet-CPU median):

  Before (1.0.3): simple 1.67 µs | rrule 1.89 µs | timezone 3.09 µs
  After  (2.0.0): simple 1.37 µs | rrule 1.60 µs | timezone 2.73 µs
  Δ:             −18% simple   | −15% rrule   | −12% timezone

Lead over icalendar 0.17:

  Before: simple 3.6× | rrule 3.8× | timezone 3.5×
  After:  simple 4.4× | rrule 4.1× | timezone 3.9×

API break: pub field type change on RawComponent / RawProperty /
VTimezone. Most consumer code compiles unchanged via Deref<Target=str>
+ PartialEq<&str>. RawProperty.value + ParsedInvite.summary stay
String (variable-length, often >24 B).

Clean sweep on parse. Note: `icalendar` has serializer / builder APIs we don't bench against because mailrs-ical's serializer surface is narrower.

#### `mailrs-rate-limit` vs `governor` 0.10 (DashMap-backed)

3-run noise-controlled median (re-measured v4 ckpt 12, 2026-06-03):

| Input | mailrs-rate-limit | governor | Winner |
|---|---:|---:|---|
| hot key, allowed | **12.6 ns** | 13.8 ns | **mailrs +9%** ✅ |
| cold key first-touch | **155 ns** | 151 ns | **TIE** (−3 % noise) |

**v4 ckpt 12 honest re-measure (2026-06-03)**: hot path lead +9 %
preserved; cold path is now essentially TIE. The prior table
(`mailrs 210 ns / governor 222 ns / +5-6 % lead`) was a single-run
measurement; 3-run on the same hardware shows both sides ~50 ns
faster than that number (rustc / DashMap / quanta minor upgrades
since 2026-05) but the cold lead narrowed inside noise. Hot
allowed remained at exactly the same ratio.

Structural advantages (GCRA TAT-in-AtomicU64, lock-free
compare_exchange_weak update, quanta clock, pre-computed
nanos_per_token) are unchanged. Cold path remains tied because
first-touch is dominated by DashMap shard lock acquisition +
allocation, which both sides share.

Caught up. The earlier 2.2× governor lead came from three sources, all of them governor's open-source homework that we hadn't done:

1. **GCRA-style storage.** Old impl stored `Bucket { tokens: f64, last_refill: u64 }` and took a `DashMap` *write lock* per check. New impl stores a single `AtomicU64` holding the theoretical-arrival-time (TAT) in monotonic nanos; reads take the DashMap shard's *read* lock and the update is a `compare_exchange_weak` loop. Multiple checks on the same key can now proceed in parallel; updates are lock-free.
2. **`quanta` clock.** `SystemTime::now()` (~10 ns syscall) → `quanta::Clock::now()` (~3-5 ns mach_absolute_time, same library governor uses). The `Duration → u128 nanos → u64` cast chain that `std::time::Instant::elapsed()` requires was the last ~5 ns; quanta returns u64-backed `Instant`s directly.
3. **Pre-computed config.** `nanos_per_token` and `burst_nanos` are computed once at construction so the hot path is integer arithmetic only.

Token-bucket semantics are preserved end-to-end — capacity/refill_rate config is identical; the GCRA encoding is an equivalent way to represent the same state. See `crates/rate-limit/src/in_memory.rs` for the implementation.

#### `mailrs-backoff` vs `exponential-backoff` 2

| Input | mailrs-backoff | exponential-backoff | Winner |
|---|---:|---:|---|
| single attempt, no jitter | 2 ns | 52 ns | **mailrs 26×** ✅ |
| single attempt, full jitter | 3 ns | 52 ns | **mailrs 17×** ✅ |
| 8-attempt chain, no jitter | 10 ns | 79 ns | **mailrs 8×** ✅ |

We're a pure function `base_delay(attempt: u32)`; `exponential-backoff` is iterator-shaped and pays setup cost per call. Different API contracts; the comparison is "how much does the typical retry loop pay per probe". Mailrs wins because we don't allocate.

#### `mailrs-smtp-proto` vs `smtp-codec` 0.2 (Rust nom-based SMTP parser)

| Command | mailrs-smtp-proto | smtp-codec | Winner |
|---|---:|---:|---|
| `EHLO mail.example.com` | **10.3 ns** | 129 ns | **mailrs 12.5×** ✅ |
| `MAIL FROM:<…> SIZE=…` | **68 ns** | 205 ns | **mailrs 3.0×** ✅ |
| `RCPT TO:<…>` | **42 ns** | 150 ns | **mailrs 3.5×** ✅ |
| `DATA` | **3.7 ns** | 14.5 ns | **mailrs 3.9×** ✅ |

**v4 round 27 (2026-05-26 — MAIL FROM / RCPT TO byte-cmp)**:
`parse_mail_from` / `parse_rcpt_to` previously allocated a
String of the entire args region just to check the 5-byte
`FROM:` / `TO:` prefix case-insensitively. With ESMTP params
(`MAIL FROM:<a@b> SIZE=4096 BODY=8BITMIME`) that args slice
can be 50+ bytes — all heap-allocated and uppercased just to
inspect five. Replaced with byte-level `eq_ignore_ascii_case`
on the prefix slice via a `starts_with_ascii_ci` helper. Drops
one heap alloc per MAIL FROM / RCPT TO command.

  Before: MAIL FROM 93.7 ns | RCPT TO 52.6 ns
  After:  MAIL FROM 68   ns | RCPT TO 42   ns
  Δ:     −27%             | −20%

Lead vs smtp-codec: MAIL FROM 2.4× → 3.0×, RCPT TO 3.1× → 3.5×.

Clean sweep. The previous DATA −25% loss was the only blemish — fixed
in v4 round 2 by killing the `verb.to_ascii_uppercase()` heap
allocation per command. For the verb-only DATA case, the per-call
String alloc was the entire wall clock (16 ns); replacing it with a
16-byte stack buffer + `match` over `&[u8]` literals drops the cost
to ~4 ns. Same pattern applied to `mech_str.to_ascii_uppercase()`
inside `parse_auth`.

Bench source: `crates/smtp-proto/benches/compare_smtp_codec.rs`. Run
`cargo bench -p mailrs-smtp-proto --bench compare_smtp_codec`.

#### `mailrs-imap-proto` vs `imap-codec` 2.0-alpha (Rust nom-based IMAP codec)

3-run noise-controlled medians (re-measured in v4 ckpt 6, 2026-06-03):

| Command | mailrs-imap-proto | imap-codec | Winner |
|---|---:|---:|---|
| `A001 SELECT INBOX` | **59 ns** | 61 ns | **mailrs +3%** (TIE) |
| `A002 FETCH 1:100 (FLAGS BODY[…])` | **104 ns** | 300 ns | **mailrs 2.87×** ✅ |
| `A003 LOGIN alice@example.com password` | **96 ns** | 110 ns | **mailrs +15%** ✅ |
| `A004 NOOP` | **32.5 ns** | 35.6 ns | **mailrs +10%** ✅ |

**v4 ckpt 6 retraction (2026-06-03)**: previous numbers (SELECT 47.8 /
FETCH 82.0 / LOGIN 78.8 / NOOP 27.8) on the mailrs side were single-run
outliers — re-measure with 3 fresh runs shows mailrs side ~15-25%
slower than claimed (imap-codec side stayed within noise). Lead
margins shrunk: SELECT +23% → TIE; FETCH 3.4× → 2.87×; LOGIN +30% →
+15%; NOOP +23% → +10%. Structural advantages still real (we hold
the lead on all 4 paths), but the absolute claims weren't reproducible
on 3-run.

The v4 round 1 changes that drove the original numbers are unchanged
and still load-bearing:

1. **Stack-buffer verb uppercase.** Replaced
   `cmd_word.to_uppercase().as_str()` (which allocates a `String` per
   command) with a 16-byte `[u8; 16]` stack buffer + manual ASCII
   uppercase loop + `match` against byte-literal arms (`b"LOGIN" =>
   ...`). Saves one heap alloc per command — dominant on short verbs
   like NOOP.
2. **Zero-intermediate-alloc `parse_login_args`.** Old impl built a
   `Vec<String>` + rolling `String` + `parts[i].clone()`, totalling
   ~5 heap allocs per LOGIN. New impl is a single byte-level forward
   pass with two allocations (the two returned `String`s — minimum
   given the public API). Same byte-token scanner pattern as imap-
   codec's `astring` parser; we can match their alloc count now.
3. The macro-`match` over `cmd_upper: &[u8]` lets LLVM lower the
   verb dispatch to a length-keyed jump-table rather than a chain
   of `eq_ignore_ascii_case` comparisons.

Bench source: `crates/imap-proto/benches/compare_imap_codec.rs`. Run
`cargo bench -p mailrs-imap-proto --bench compare_imap_codec` to
reproduce.

### Cross-language (`bench-harness/`)

Sub-process bench harness in `bench-harness/` runs the same operations
across Rust + C + Go on identical corpus files. C and Go runners are
best-effort — skipped if the toolchain / library isn't installed.

First end-to-end run (2026-05-23, Darwin 25.5.0 arm64):

| Scenario | Rust (mailrs) | C | Go |
|---|---:|---:|---:|
| RFC 5322 read + Subject + From | **46 ns** | n/a | net/mail: 1440 ns (**mailrs 31× faster**) |
| SPF parse — simple | **65 ns** | libspf2: not on brew (source build) | n/a |
| SPF parse — complex | **401 ns** | libspf2: not on brew (source build) | n/a |
| DKIM-Signature parse | **431 ns** | opendkim: not on brew (source build) | n/a |
| iCalendar parse | **1.76 µs** | libical 4.0: 7032 ns (**mailrs 4.0× faster**) | n/a |
| MIME tree parse (simple msg) | **601 ns** | GMime: not yet wired | n/a |

Two fully-paired cross-language data points so far, both wins for
mailrs by margins that match the "modern Rust implementation,
performance-first" positioning:

- **vs. Go stdlib `net/mail.ReadMessage`** — mailrs-rfc5322 is **31×
  faster** doing the same "read message + extract Subject + From"
  workload.
- **vs. C library `libical` 4.0** (the 20+ year reference impl
  powering Evolution, GNOME Calendar, etc.) — mailrs-ical parses the
  same iCalendar input **4.0× faster**.

C library wiring is best-effort. libspf2 and opendkim aren't on brew;
adding them requires a source build. The C runner stubs in
`bench-harness/c/` are ready — any contributor with a built libspf2
can drop in the binary and `scripts/run-all.sh` will pick it up.

### `mailrs-smtp-proto` (criterion, `cargo bench -p mailrs-smtp-proto`)

| Path | Median | Notes |
|---|---:|---|
| `parse_command/EHLO` | **6 ns** | was 22 ns; v4 round 2 killed `verb.to_ascii_uppercase()` heap alloc |
| `parse_command/DATA` | **4 ns** | was 22 ns (and 16 ns vs smtp-codec 12 ns → loss); v4 round 2 = **−82%** |
| `parse_command/RCPT_TO` | **32 ns** | was 70 ns; same verb-buffer change |
| `parse_command/MAIL_FROM` | **66 ns** | was 103 ns; same |
| `parse_command/AUTH_PLAIN` | **11 ns** | |
| `format_ehlo_response` | **38 ns** | was 307 ns; commit `19aa482` replaced `write!`-macro dispatch with direct `push_str` of `&str` segments for **−89%** measured (~9× faster) |
| `address/is_valid_typical` | **6 ns** | |
| `address/split_typical` | **7 ns** | |
| `unstuff_data/1024b` | **168 ns** | was 371 ns; v4 round 1 (ckpt 5, 2026-06-03) memchr scan **−55% / 2.2×** |
| `unstuff_data/10240b` | **2.82 µs** | was 5.00 µs; same change **−44% / 1.8×** (3.6 GB/s) |
| `unstuff_data/102400b` | **20.85 µs** | was 40.85 µs; same change **−49% / 2.0×** (4.9 GB/s) |

**v4 round 1** (2026-06-03, ckpt 5): replaced the
`iter().position(|b| b == b'\n')` line-scan in `unstuff_data` with
`memchr::memchr`. `unstuff_data` is the SMTP DATA dot-stuffing
remover — called once per inbound message body in
`server::smtp_session::events::data`, so the win compounds across
bulk inbound. ~2× across all measured sizes (1 KB / 10 KB / 100 KB).
Other ops on this stone re-measured during ckpt 5 and were
within ±5 % noise of their prior numbers (the verb-buffer wins
from v4 round 2 are stable).

### `mailrs-smtp-codec` (criterion, `cargo bench -p mailrs-smtp-codec`)

Tokio `Decoder` / `Encoder` for the RFC 5321 SMTP wire format —
switches between line-oriented command mode (CRLF-terminated,
≤1024 octets) and DATA mode (raw bytes until `CRLF.CRLF`). The
two helpers `has_smuggle_sequence` and `normalize_line_endings`
are the cost centres in DATA mode and run on every accepted
message body in Strict and Permissive smuggle-protection modes
respectively.

**Label: first-in-Rust** — no other Rust crate implements
SMTP-smuggling-aware framing as a published primitive
(`tokio_util::codec::LinesCodec` does generic `\n` line framing
without smuggle awareness; stalwart's `smtp-codec` is a parser,
not a Tokio codec).

| Path | Median | Throughput | Notes |
|---|---:|---:|---|
| `has_smuggle_sequence/safe` (10 B) | **3.96 ns** | — | tiny-input regression (+25% vs naive loop) — memchr setup cost dominates; not a prod shape |
| `has_smuggle_sequence/clean_1024b` | **12.7 ns** | 81 GB/s | was 316 ns; v4 round 1 memchr-anchored scan **−96 % / 25× faster** |
| `has_smuggle_sequence/clean_10240b` | **95 ns** | 108 GB/s | was 2.9 µs; **−97 % / 30×** |
| `has_smuggle_sequence/clean_102400b` | **907 ns** | 113 GB/s | was 28.5 µs; **−97 % / 31×** — close to memchr SIMD ceiling |
| `normalize_line_endings/lf_only` (12 B) | **55 ns** | — | unchanged — alloc-bound on tiny input |
| `normalize_line_endings/bare_lf_1024b` | **152 ns** | 6.7 GB/s | was 701 ns; v4 round 1 memchr2 + chunked extend **−78 %** |
| `normalize_line_endings/bare_lf_10240b` | **3.56 µs** | 2.9 GB/s | was 8.86 µs; **−60 %** |
| `normalize_line_endings/bare_lf_102400b` | **18.8 µs** | 5.5 GB/s | was 67.9 µs; **−72 % / 3.6×** |
| `decode/command/ehlo` | **78 ns** | — | `BytesMut::split_to` + UTF-8 lossy decode dominate |
| `decode/command/mail_from` | **80 ns** | — | longest of the 4 commands measured |
| `decode/command/data` | **64 ns** | — | shortest — 6-byte frame |
| `decode/data/permissive_1024b` | **389 ns** | 2.6 GB/s | was 963 ns; **−60 %** |
| `decode/data/strict_1024b` | **303 ns** | 3.4 GB/s | was 873 ns; **−65 %** |
| `decode/data/off_1024b` | **93 ns** | 11 GB/s | was 408 ns; **−77 %** — `find_data_terminator` memchr-anchored |
| `decode/data/permissive_102400b` | **52.1 µs** | 2.0 GB/s | was 104 µs; **−50 %** — per-message hot path on Permissive default |
| `decode/data/strict_102400b` | **39.9 µs** | 2.6 GB/s | was 93.6 µs; **−57 %** |
| `decode/data/off_102400b` | **15.7 µs** | 6.5 GB/s | was 46.4 µs; **−69 % / 3×** |

**v4 round 1** (2026-06-02, ckpt 1): rewrote three byte-by-byte
scanners as memchr-anchored helpers.
- `has_smuggle_sequence`: anchor on `\n` then verify the local
  smuggle shape — LF is rare so SIMD memchr prunes >99 % of
  bytes on clean inputs.
- `normalize_line_endings`: `memchr2(b'\r', b'\n', ...)` for the
  next line-ending, then `extend_from_slice` on the clean run
  between anchors (memcpy under the hood instead of one
  `push()` per byte).
- `find_data_terminator`: anchor on `.` (the rarest byte in
  `\r\n.\r\n`) instead of `windows(5)` byte-by-byte.

Bench source: `crates/smtp-codec/benches/smtp_codec.rs`. Run
`cargo bench -p mailrs-smtp-codec`.

### `mailrs-imap-codec` (criterion, `cargo bench -p mailrs-imap-codec`)

Tokio `Decoder` / `Encoder` for the RFC 9051 IMAP wire format —
switches between line mode (CRLF-terminated commands and
responses) and literal mode (raw byte-counted payloads as
declared by the `{N}` marker, used for APPEND, FETCH BODY[…],
passwords with special chars). Stateful: caller toggles literal
mode by calling `expect_literal(size)` after parsing the marker
from the protocol layer above.

**Label: first-in-Rust** on literal-aware IMAP framing —
`tokio_util::codec::LinesCodec` does only generic `\n` line
framing, and `imap-codec` (stalwart's crate) is a command /
response parser, not a Tokio codec. Nothing else combines line
framing + byte-counted literals as a published primitive.

| Path | Input | Median | Throughput | Notes |
|---|---|---:|---:|---|
| `decode/line/noop` | 11 B (`A001 NOOP\r\n`) | **65 ns** | — | short command, alloc-bound |
| `decode/line/login` | 22 B (`a001 LOGIN user pass\r\n`) | **72 ns** | — | matches the v4 baseline LOGIN row (70 ns) |
| `decode/line/select` | 19 B (`a002 SELECT INBOX\r\n`) | **67 ns** | — | |
| `decode/line/fetch_long` | 160 B (FETCH response with BODY metadata) | **107 ns** | 1.5 GB/s | line scaling reaches SIMD memchr floor |
| `decode/line/bare_cr_skip` | 24 B with 5 embedded bare `\r` | **76 ns** | — | exercises the memchr restart loop (RFC 9051 requires bare CR to be skipped) |
| `decode/literal/32b` | 32 B + CRLF | **62 ns** | — | minimal literal overhead |
| `decode/literal/1024b` | 1 KB + CRLF | **87.5 ns** | 12 GB/s | `BytesMut::split_to` + `to_vec` — single memcpy |
| `decode/literal/102400b` | 100 KB + CRLF | **13.2 µs** | **7.7 GB/s** | **memcpy ceiling** — split_to is zero-copy share, to_vec is the bound |
| `encode/short_12b` | 12 B | **38 ns** | — | one `extend_from_slice` to `BytesMut` |
| `encode/long_140b` | 140 B | **39.4 ns** | — | encode does not scale with payload — `BytesMut::extend_from_slice` is memcpy bound, dominated by setup overhead |

**v4 round 1** (2026-06-02, ckpt 2): **Case A** — no exploitable
hot path. The line scanner already uses `memchr` (added during
v3 cycle); the literal path is a thin wrapper over
`BytesMut::split_to`. All public ops sit within ~30 % of the
hardware floor (memchr SIMD on the scan side, memcpy on the
copy side). Work in this ckpt was bench coverage + docs:

- bench coverage expanded from 1 op (`LOGIN`) to **10 ops**
  across line / literal / bare-CR-skip / encode dimensions
- `perf_gate.rs` adds a literal-decode budget (100 KB payload,
  100 µs gate, ~7× headroom)
- `PERFORMANCE.md` (this section) + `BUDGETS.md` populated with
  measured numbers

**v4 round 2 — explored, rejected** (2026-06-02): tested
replacing the line-mode `String::from_utf8_lossy(&line).into_owned()`
with `String::from_utf8(line.to_vec())` + lossy fallback. Standalone
probe showed 1.9-2.5× speed-up on the conversion step alone, but
in the full `decode/line/...` context the result was *mixed*:

| op | input | before | after | delta |
|---|---|---:|---:|---:|
| `decode/line/fetch_long` | 160 B | 107 ns | 70 ns | **−35 % ✓** |
| `decode/line/login` | 22 B | 72 ns | 79 ns | **+10 % ✗** |

Short commands (`NOOP`, `LOGIN`, `SELECT`) are the server's actual
hot path (clients send them far more often than servers emit long
FETCH responses), so the short-command +10 % regression
outweighs the long-line −35 % win for the per-message accounting.
Reverted. Recorded here so future v4 rounds don't re-explore.

What would change the verdict: if a future profile shows long FETCH
responses dominate the server's IMAP CPU budget (e.g. a workload
shift to bulk `FETCH 1:* BODY[]`), revisit. The probe data is real
— the trade-off is workload-dependent.

Bench source: `crates/imap-codec/benches/imap_codec.rs`. Run
`cargo bench -p mailrs-imap-codec`.

### `mailrs-imap-format` (criterion, `cargo bench -p mailrs-imap-format`)

| Path | Median | Notes |
|---|---:|---|
| `format_imap_flags/seen+answered` | **19 ns** | was 27.8 ns then 12.9 ns; current re-measure 19 ns sits between — noise variance (the v4-cycle 12.9 ns figure was a quiet-CPU outlier). Structural win (no `Vec::push` + `join`) is unchanged. |
| `parse_imap_flags/seen answered` | **15 ns** | matches the v4-cycle 16.1 ns figure within noise; `eq_ignore_ascii_case` against compile-time `&[u8; N]` targets, still load-bearing |
| `format_internal_date` | **177 ns** | dominated by `chrono` `from_timestamp` + format; squeeze deferred (would require an in-house date formatter) |
| `extract_header_section/body_1kb` | **78 ns** | was 130 ns; v4 round 1 (ckpt 7, 2026-06-03) memchr-anchored separator scan **−40% / 1.66×** |
| `extract_header_section/body_5kb` | **79 ns** | was 129 ns; same change **−39% / 1.65×** (constant in body size — scanner stops at separator) |
| `extract_header_section/body_20kb` | **80 ns** | was 128 ns; same change **−37% / 1.59×** |
| `extract_body_section/body_1kb` | **95 ns** | was 132 ns; same change **−28% / 1.39×** (scan + Vec alloc for body output) |
| `extract_body_section/body_5kb` | **122 ns** | was 158 ns; **−23% / 1.30×** |
| `extract_body_section/body_20kb` | **1.37 µs** | was 1.41 µs; **−3% (noise)** — at 20 KB the output `Vec::to_vec` memcpy dominates, scan cost amortizes away |
| `find_line_offset/line_1` | **2.3 ns** | was 17.7 ns; **−87% / 7.7×** — short-skip case; memchr's SIMD startup overhead amortizes immediately on input → just 1 LF away |
| `find_line_offset/line_50` | **319 ns** | new bench coverage (no prior baseline) — typical FETCH `BODY[TEXT]<N.M>` partial-fetch shape |
| `find_line_offset/line_120` | **794 ns** | new bench coverage |

**v4 round 1** (2026-06-03, ckpt 7): swapped three byte-by-byte
scans in `mime.rs` for `memchr`-anchored helpers.

1. `extract_header_section` + `extract_body_section`: shared
   `find_separator_end` helper anchors on `\n` (rare in textual
   headers), checks for both `\r\n\r\n` (canonical) and `\n\n`
   (bare-LF MTAs) at each candidate. Replaces the
   `windows(4).position` + `windows(2).position` fallback pair.
2. `find_line_offset`: memchr `\n` in the line-counter loop in
   place of `body[pos..].iter().position(|&b| b == b'\n')`.

All three are per-FETCH hot paths — every IMAP `FETCH BODY[HEADER]`
/ `FETCH BODY[TEXT]` / `FETCH BODY[TEXT]<N.M>` touches one of them.

Bench coverage: from 3 ops (`format_imap_flags` / `parse_imap_flags`
/ `format_internal_date`) to **12 ops** — `extract_header_section`,
`extract_body_section`, `find_line_offset` each across 3 input
shapes (1 KB / 5 KB / 20 KB body for the section extractors; line
1 / 50 / 120 for `find_line_offset`). Previously this was a
prod-hot-but-not-benched gap (same pattern as `smtp-proto::unstuff_data`
in ckpt 5).

### `mailrs-smtp-client` (criterion, `cargo bench -p mailrs-smtp-client`)

Re-measured v4 ckpt 16 (2026-06-03), 3-run honest medians:

| Path | Median | Notes |
|---|---:|---|
| `sort_mx_records(20)` | **12 ns** | MX priority sort |
| `parse_response/short` | **24 ns** | was 27 ns; matches baseline within noise. v4 round 5 unrolled 3-digit byte code parse still load-bearing |
| `parse_response/long_ehlo_10_lines` | **181 ns** | was 257 ns; numbers improved further (likely rustc / stdlib `lines()` improvement) |
| `dot_stuff(5 KB no dots)` | **~1.4 µs** | passthrough fast-path |
| `dot_stuff(5 KB with dots)` | **~1.6 µs** | allocates new Vec to escape |

**v4 ckpt 16** (2026-06-03): Case A verified — `grep iter().position` /
`.windows(N)` / `push_str(&format!(...))` / `String::replace` in
src/ → 0 hits. The crate is 3k LOC across 6 files but ~2.3k of
those are I/O-bound code paths (mx.rs MX resolver / dane.rs DANE
TLSA verification / connection.rs TCP+TLS connection / tls_outcome.rs)
— async I/O dominates, no memchr territory. The remaining byte-level
hot path is `parse_response` (response.rs) which already uses stdlib
`str::lines()` (internally memchr-based) plus an inlined `parse_code3`
byte-level fast path for the 3-digit SMTP reply code.

**P1 round complete with this ckpt: 9 stones (dkim through smtp-client)
swept; 5 brought structural wins (memchr / write!() / pre-size / etc.),
4 verified as Case A with honest re-measure.**

### `mailrs-imap-proto` (criterion, `cargo bench -p mailrs-imap-proto`)

Re-measured in v4 ckpt 6 (2026-06-03), 3-run honest medians:

| Path | Median | Notes |
|---|---:|---|
| `parse_command(LOGIN)` | **88 ns** | was previously claimed ~123 ns — actually faster than the over-cautious claim |
| `parse_command(SELECT)` | **56 ns** | |
| `parse_command(FETCH_uid_range)` | **100 ns** | `FETCH 1:1000 (FLAGS BODY.PEEK[HEADER])` |
| `parse_command(complex UID SEARCH)` | **164 ns** | was claimed ~155 ns (within noise) |
| `sequence_set/parse_simple` | **134 ns** | `"1,3,5,7,9,11"` |
| `sequence_set/parse_ranges` | **110 ns** | `"1:100,200:300,400:500,*"` |
| `sequence_set_to_uids` (~2 K UIDs) | **5.8 µs** | was claimed ~3.0 µs — single-run outlier; real cost is `(1..=1000).collect() + (2000..=3000).collect() + sort + dedup` dominated by stable-sort + flat_map alloc. **Sort/dedup are necessary** for correctness; bench-name "n4001" is historical (real count is ~2002 elements). |

**v4 ckpt 6 — Case A verified, 2 over-claims retracted.** No
exploitable hot path beyond what v4 round 1 (stack-buffer verb
uppercase, zero-intermediate-alloc parse_login_args, byte-cmp verb
dispatch) already shipped. grep `iter().position` / `.windows(N)` in
src/ → 0 hits. The remaining `.to_string()` calls in `parse.rs` and
`search.rs` are API-essential (`ImapCommand` variants own `String`
fields; can't be `&str` without lifetimes leaking through the
public API).

### `mailrs-jmap` (criterion, `cargo bench -p mailrs-jmap`)

| Path | Median | Notes |
|---|---:|---|
| `keywords_to_flags` | **~5.6 ns** | bitmask conversion |
| `dispatch Email/query` | **~2.4 µs** | single dispatch w/ in-memory store |
| `dispatch_request multi-call back-ref` | **~10.4 µs** | full JMAP `Request` with `#ref` |

### `mailrs-maildir` — Maildir delivery + flag parsing (criterion, M-series Mac, release)

Re-measured v4 ckpt 13 (2026-06-03), 3-run honest medians:

| Path | Median | Notes |
|---|---:|---|
| `parse_flags/empty` | **1.74 ns** | byte-iter scan over the cur/new entry suffix — already at noise floor |
| `parse_flags/seen_only` | **1.74 ns** | |
| `parse_flags/all_standard` | **1.75 ns** | |
| `deliver_loop/n=1` | **4.6 ms** | fs syscall bound (open + write + fsync + rename) — the floor is the filesystem, not the parser |
| `deliver_loop/n=8` | **39 ms** | per-message loop overhead linear in N |
| `deliver_batch/n=8` | **11.4 ms** | 8 messages batched in one fsync, **3.4× faster than loop** — the prior batch-fsync win still load-bearing |
| `deliver_batch/n=64` | **15.8 ms** | batch overhead amortizes — 20× per-message savings over deliver_loop at scale |

**v4 ckpt 13** (2026-06-03): Case A verified — `grep iter().position` /
`.windows(N)` / `push_str(&format!(...))` in src/ → 0 hits. The
crate is a 433-line single-file `lib.rs` with no string parsing
beyond the trivial Maildir flag-suffix walk (already at ns-noise
floor); the deliver hot path is fs-syscall bound and the existing
batch-fsync optimization already gives the structural win at scale.

### `mailrs-mailbox` (criterion, `cargo bench -p mailrs-mailbox`)

Re-measured v4 ckpt 14 (2026-06-03), 3-run honest medians:

| Path | Median | Notes |
|---|---:|---|
| `add_flags` hot path | **52 ns** | DashMap entry update |
| `extract_message_id(short header)` | **59 ns** | was ~150 ns pre-v4; memchr-anchored byte-level header walk replaced `from_utf8_lossy(data).lines()` |
| `extract_message_id(long header)` | **123 ns** | 20+ header lines; bounded by header count not body length |
| `mailbox_status` (1k messages) | **468 ns** | fixture-impl walks DashMap; PG impl pushes into SQL |
| `insert_message/first_insert` | **288 ns** | fixture insert + DashMap update |
| `insert_message/into_1k_mailbox` | **63 µs** | fixture cost — clones Message rows |
| `query_messages/by_mailbox_first_50` | **164 µs** | fixture cost — see PG comparison in README |
| `query_messages/text_match_1k` | **150 µs** | same — fixture-only; PG impl pushes search into SQL `WHERE` clause |

**v4 ckpt 14** (2026-06-03): Case A verified — `grep iter().position` /
`.windows(N)` / `push_str(&format!(...))` / `String::replace` in
src/ → 0 hits. The crate is 7k LOC but ~90 % of it is the PG impl
(7 `pg_*` submodules) — those are SQL queries and bind clauses, no
memchr territory. The remaining hot path is `threading::extract_header_value`
which has been memchr-anchored since the original v4 squeeze cycle.
Numbers in this table re-confirmed 3-run; the prior `~55 ns` and
`~120 µs` rounded values match 3-run honest within noise.

### `mailrs-rate-limit` (criterion, `cargo bench -p mailrs-rate-limit`)

Re-measured v4 ckpt 12 (2026-06-03), 3-run honest medians:

| Path | Median | Notes |
|---|---:|---|
| `evaluate_bucket/allowed` (pure math) | **1.55 ns** | GCRA TAT integer arithmetic, no I/O |
| `evaluate_bucket/denied_no_refill` | **1.57 ns** | |
| `check_hot_key/sync` | **12.9 ns** | bypass async trait; was claimed ~33 ns — under-claim retracted, real is 2.5× faster |
| `check_hot_key/async` | **64 ns** | through `RateLimitStore` trait; was claimed ~84 ns — under-claim retracted |
| `check_cold_key/first_touch` | **~150 ns** | DashMap insert path |
| `cleanup_stale(10k)` | **~145 µs** | batch scan + retain (was ~100 µs in v4-round-N claim; current 3-run honest) |

**v4 ckpt 12** (2026-06-03): Case A verified — grep `iter().position` /
`.windows(N)` / `push_str(&format!(...))` in src/ → 0 hits. The
crate has no string parsing (token-bucket math + DashMap only) so
none of the memchr / write!() patterns from earlier ckpts apply.
Stone-level numbers re-measured 3-run honest; the prior table was
v4-round-N vintage and under-claimed (real numbers are ~2× faster
than the table reported, similar to the spf under-claim in ckpt 9).

### `mailrs-shield` (criterion, `cargo bench -p mailrs-shield`)

Re-measured v4 ckpt 18 (2026-06-03), 3-run honest medians:

| Path | Median | Notes |
|---|---:|---|
| `dnsbl/reverse_ipv4` | **47 ns** | reverse IPv4 octet string build for DNSBL query |
| `dnsbl/interpret_spamhaus` | **524 ps** | bit-interpretation of Spamhaus A-record octets |
| `greylist/evaluate_first_seen` | **479 ps** | first-touch decision |
| `greylist/evaluate_retry` | **677 ps** | retry-window comparison |
| `greylist/triplet_key` | **25 ns** | was 120 ns pre-v4; commit `d0c5941` replaced `format!` with pre-sized `String::with_capacity + push_str` (5× faster). Per inbound message on the greylist hot path |
| `ptr_score_from_names(match)` | **75 ns** | FCrDNS score eval |
| `ptr_score_from_names(no match)` | **205 ns** | DNS-mismatch slow path (extra HashSet ops) |

**v4 ckpt 18** (2026-06-03): Case A verified — `grep iter().position` /
`.windows(N)` / `push_str(&format!(...))` / `String::replace` in
src/ → 0 hits. The crate is 548 LOC across 4 files (lib / dnsbl /
ptr / greylist); all hot ops are at picosecond-to-100ns range
(interpret_spamhaus and greylist/evaluate at ~500 ps are essentially
at the criterion measurement floor). The v4-period `triplet_key`
optimisation is durable. Numbers re-confirmed against baseline.

### `mailrs-spf` — RFC 7208 SPF verifier (criterion, M-series Mac, release)

Re-measured v4 ckpt 9 (2026-06-03), 3-run honest medians:

| Path | Median |
|---|---:|
| `Record::parse` (simple `v=spf1 ip4 -all`) | **46 ns** |
| `Record::parse` (complex 8-mechanism record) | **244 ns** |
| `verify` pass path (no real DNS) | **175 ns** |

Run: `cargo bench -p mailrs-spf --bench spf`. Production `verify` is
dominated by DNS round-trips (5-50 ms); the bench numbers above are
the pure CPU portion.

**v4 ckpt 9** (2026-06-03): Case A verified — `grep iter().position` /
`.windows(N)` in src/ → 0 hits, hot scans already memchr-anchored
since v3 cycle. The under-claim direction here is unusual: previous
table (82 ns / 484 ns / 244 ns) was the v4-round-4 vintage; cumulative
gains from rounds 12/13/20 (CompactString for domain fields, byte-level
mechanism dispatch) dropped the real numbers another ~40-50% but the
stone-level table wasn't refreshed. Vs-`mail-auth` comparison section
was already up-to-date and unchanged.

### `mailrs-dmarc` — RFC 7489 DMARC verifier + aggregate report (criterion, M-series Mac, release)

3-run honest medians, v4 ckpt 11 (2026-06-03):

| Path | Median | Notes |
|---|---:|---|
| `generate_xml/n10` | **7.74 µs** | was 13.2 µs; v4 ckpt 11 `write!()` rewrite **−41 % / 1.71×** |
| `generate_xml/n500` | **275 µs** | was 533 µs; same change **−48 % / 1.94×** (linear scaling) |
| `format_report_email` | **83 µs** | unchanged — already on `mailrs-mail-builder` since v8 ckpt 3 |
| `extract_rua_typical` | **70 ns** | tag-list scan via stdlib `split(';')` (stdlib uses memchr internally for `char` patterns) — already optimal |

**v4 ckpt 11** (2026-06-03): two anti-patterns in
`generate_dmarc_report_xml`:

1. **`push_str(&format!(...))` cascade** — every record emitted via
   ~15 `xml.push_str(&format!(...))` calls. Each one allocates a
   throwaway intermediate `String` from the `format!` macro just
   to memcpy it into `xml` and drop. Replaced with `write!(xml,
   ...)` macros that format directly into the destination — no
   intermediate allocation. Pre-sized the destination
   `String::with_capacity(512 + n_records * 600)` so the growth-
   doubling re-allocs are gone too.

2. **`escape_xml` 4×`String::replace` chain** — `s.replace('&', ...).
   replace('<', ...).replace('>', ...).replace('"', ...)` allocates
   4 intermediate `String`s per field, even when no escape is
   needed (which is the typical case — domains, IPs, result enums
   are all in the safe character set). Replaced with an
   `XmlEscape(&str)` newtype that implements `Display`: fast path
   `f.write_str(self.0)` when no special chars are present, slow
   path char-iter with escape on the fly. The old `escape_xml`
   function is kept `#[cfg(test)]` only so the 4 existing escape
   tests don't need to change.

Combined effect on the per-report XML emission: 1.71× on small
records, 1.94× on large reports where the per-record cost
dominates. `format_report_email` (gzip + multipart build via
`mailrs-mail-builder`) is unchanged since the `format_dmarc_report_xml`
output already feeds into the existing builder path that was
optimized in v8 ckpt 3.

### `mailrs-arc` — RFC 8617 ARC verifier (criterion, M-series Mac, release)

Re-measured v4 ckpt 10 (2026-06-03), 3-run honest medians:

| Path | Median | Notes |
|---|---:|---|
| `parse/aar` | **27 ns** | ARC-Authentication-Results parse |
| `parse/ams` | **541 ns** | ARC-Message-Signature parse |
| `parse/as` | **277 ns** | ARC-Seal parse |
| `chain/extract_two_hop` | **2.49 µs** | was 3.90 µs; v4 ckpt 10 memchr rewrite **−36 % / 1.57×** |

**v4 ckpt 10** (2026-06-03): replaced two pre-v4 byte-by-byte
scanners in `crates/arc/src/chain.rs`:

1. **`unfold_headers` rewrite** — the prior implementation built a
   `Vec<Vec<u8>>` of line slices upfront (one heap allocation per
   header line), then per-header allocated a `Vec<u8>` value buffer
   and called `Vec::remove(0)` in a loop to skip leading WSP
   (O(n²) shifting per continuation). The memchr-anchored rewrite
   walks `block` once with `memchr(\n)` to find line bounds, uses
   `memchr(:)` to find the name/value separator, and advances slice
   pointers instead of allocating + shifting. The common
   single-line header case takes the no-continuation fast path and
   does exactly one allocation (the value `String`).

2. **`take_header_block` memchr-ified** — the prior `find_subseq`
   helper was an O(N·M) `windows`-style walk run twice per call
   (once for `\r\n\r\n`, once for `\n\n`). Replaced with a single
   memchr(`\n`) scan that checks both shapes at each candidate
   position.

3. **Removed dead `find_subseq` helper** after take_header_block
   migrated off it.

`chain/extract_two_hop` covers both functions on a representative
6-ARC-header + From shape — the prod hot path for every inbound
message that carries an ARC chain (forwarded / mailing-list mail).
40 lib tests pass; algorithm is bytewise-identical to the prior
implementation (semantic-equivalent rewrite).

### `mailrs-backoff` — exponential backoff with optional jitter (criterion, M-series Mac, release)

| Path | Median |
|---|---:|
| `base_delay(attempt=3)` | **~8 ns** |
| `delay(attempt=3, Jitter::None)` | **~23 ns** |
| `delay(attempt=3, Jitter::Equal)` | **~31 ns** |
| `delay(attempt=3, Jitter::Full)` | **~11 ns** |
| `delay(attempt=100, capped)` | **~10 ns** |
| `should_give_up` | **<1 ns** |

Run: `cargo bench -p mailrs-backoff --bench backoff`. Generic
exponential-backoff primitive with AWS-style jitter taxonomy
(None/Equal/Full); zero runtime deps, caller supplies seed.

### `mailrs-clamav` — ClamAV TCP INSTREAM client (criterion, M-series Mac, release)

CPU portion only — `scan` itself is network-bound (10-30 ms for a
localhost clamd scan of a typical 100 KB payload).

| Path | Median |
|---|---:|
| `parse_response` (clean) | **~9 ns** |
| `parse_response` (virus, short name) | **~60 ns** |
| `parse_response` (virus, long name) | **~78 ns** |
| `parse_response` (error reply) | **~49 ns** |
| `parse_response` (empty input) | **~21 ns** |

Run: `cargo bench -p mailrs-clamav --bench clamav`. Extracted from
server's content_scan.rs; server re-exports `scan_clamav` +
`parse_clamav_response` for back-compat with existing call sites.

### `mailrs-dnsbl` — RFC 5782 DNSBL primitive (criterion, M-series Mac, release)

| Path | Median |
|---|---:|
| `reverse_ipv4` | **~45 ns** |
| `dnsbl_query` (~20-char zone) | **~17 ns** |
| `interpret_spamhaus` (Sbl reply) | **~1.15 ns** |
| `interpret_spamhaus` (non-127.x → Clean) | **~1.22 ns** |
| `DnsblCache` is_empty + len roundtrip | **~8.7 ns** |
| `DnsblResult` eq | **~720 ps** |

Run: `cargo bench -p mailrs-dnsbl --bench dnsbl`. Carved out of
`mailrs-shield` for users who only need DNSBL — same code, own crate.
`mailrs-shield` 1.0.4 re-exports the public surface unchanged.

### `mailrs-webhook-signature` — HMAC-SHA256 webhook signing (criterion, M-series Mac, release)

| Path | Median |
|---|---:|
| `sign` (32-byte payload) | **~420 ns** |
| `sign` (1 KB payload) | **~1.6 µs** |
| `sign` (100 KB payload) | **~92 µs** |
| `verify` (correct path) | **~690 ns** |
| `verify` (wrong secret, constant-time) | **~650 ns** |
| `verify_any` (2 secrets, first matches) | **~700 ns** |
| `verify_any` (2 secrets, second matches) | **~915 ns** |
| `format_header` | **~36 ns** |
| `parse_header` (with prefix) | **~16 ns** |

Run: `cargo bench -p mailrs-webhook-signature --bench signing`.
Constant-time HMAC compare via `hmac::Mac::verify_slice`. Generic
GitHub/Stripe-style webhook auth primitive; pairs with any HTTP
outbox.

### `mailrs-rfc2231` — MIME parameter encode + decode (criterion, M-series Mac, release)

| Path | Median |
|---|---:|
| `encode_param` (ASCII, legacy quoted) | **30 ns** |
| `encode_param` (Japanese, extended) | **128 ns** |
| `encode_param` (60-char Japanese filename) | **448 ns** |
| `decode_param_value` (legacy quoted) | **9 ns** |
| `decode_param_value` (legacy bareword) | **6 ns** |
| `decode_param_value` (UTF-8 extended) | **100 ns** |
| `decode_param_value` (ISO-8859-1 extended) | **133 ns** |

Run: `cargo bench -p mailrs-rfc2231 --bench params`. Pairs with
mailrs-rfc2047 to cover the full MIME header encoding suite.

### `mailrs-srs` — Sender Rewriting Scheme (criterion, M-series Mac, release)

| Path | Median |
|---|---:|
| `rewrite` (ASCII sender) | **171 ns** |
| `reverse` (success, in window) | **208 ns** |
| `reverse` (wrong secret, constant-time HMAC compare) | **127 ns** |
| `reverse` (malformed input, early exit) | **11 ns** |

Run: `cargo bench -p mailrs-srs --bench srs`. The constant-time HMAC
byte compare is verified inline — the timing difference between
success and wrong-secret paths is from the success path additionally
allocating the recovered "local@domain" String; the actual byte
comparison is constant-time.

### `mailrs-auth-guard` — failed-auth tracker (criterion, M-series Mac, release)

| Path | Median |
|---|---:|
| `check` — empty map (success path) | **43 ns** |
| `check` — below threshold | **46 ns** |
| `check` — IP locked out | **51 ns** |
| `record_failure` — fresh key | **127 ns** |
| `record_failure` — repeat | **75 ns** |
| `record_success` — clear counter | **62 ns** |

Run: `cargo bench -p mailrs-auth-guard --bench guard`. The success
path (`check` → `Allowed`) is the hot one — every legitimate login
goes through it, two DashMap reads + no allocation.

### `mailrs-rfc2047` — encoded-word decoder (criterion, M-series Mac, release)

| Path | Median | Notes |
|---|---:|---|
| `decode/ascii_passthrough` | **25 ns** | fast-path: scan for `=?`, return `Cow::Borrowed` |
| `decode/utf8_B_simple` | **66 ns** | UTF-8 Base64 short subject |
| `decode/utf8_Q_simple` | **78 ns** | UTF-8 Quoted-printable short subject |
| `decode/iso_2022_jp` | **154 ns** | ISO-2022-JP via `encoding_rs` (Japanese subjects) |
| `decode/mixed_ascii_and_encoded` | **104 ns** | `Re: =?…?= text` shape |

### Subject extraction: `mailrs-rfc2047` vs `mail-parser` full parse

| Subject form | mail-parser | mailrs-rfc2047 (post-rfc5322 header lookup) | speedup |
|---|---:|---:|---:|
| ASCII | 442 ns | **28 ns** | **15.8×** |
| UTF-8 Base64 encoded | 439 ns | **110 ns** | **4.0×** |

Run: `cargo bench -p mailrs-rfc2047 --bench decode`.

### `mailrs-rfc5322` vs `mail-parser` — comparative bench

| Operation | body size | mailrs-rfc5322 | mail-parser 0.11 | speedup |
|---|---:|---:|---:|---:|
| Subject + From lookup | 1 KB | **83 ns** | 2629 ns | **31.7×** |
| Subject + From lookup | 5 KB | **84 ns** | 3727 ns | **44.4×** |
| Subject + From lookup | 20 KB | **84 ns** | 7682 ns | **91.5×** |
| Target at end of 50 headers (worst case) | — | **436 ns** | n/a | n/a |
| body offset locate | 1 KB | **104 ns** | 2554 ns | **24.6×** |
| body offset locate | 5 KB | **105 ns** | 3654 ns | **34.7×** |
| body offset locate | 20 KB | **105 ns** | 7674 ns | **73.0×** |
| Received-chain walk (3 hops) | — | **127 ns** | 3691 ns | **29.1×** |

`mailrs-rfc5322` is **constant-time in body size** because the scanner
stops at the header/body boundary. `mail-parser` is linear in body
size because it builds the full Message tree. For an SMTP receive
pipeline reading 2-5 headers per message, that's 6-7 µs/msg saved on
20 KB messages — at 1000 msg/sec, **6-7 ms/sec of CPU freed.**

**v4 round 1** (2026-06-02, ckpt 3): swapped two `iter().position()`
byte-by-byte scans in `header.rs` for `memchr::memchr` —
`find_unfolded_line_end` (per-header CRLF scan, dominant cost) and
`parse_header_line` (per-line colon find). Wins:

| Op | Before | After | Δ |
|---|---:|---:|---:|
| header lookup 1 KB body | 222 ns | 83 ns | **−63 % / 2.7×** |
| header lookup 20 KB body | 223 ns | 84 ns | **−62 % / 2.7×** |
| body locate 20 KB body | 228 ns | 105 ns | **−54 % / 2.2×** |
| received-chain walk | 327 ns | 127 ns | **−61 % / 2.6×** |
| worst-case (target at end of 50 short headers) | 451 ns | 436 ns | −3 % (noise; short headers, memchr setup ≈ scan benefit) |

The speedup vs mail-parser tripled (11-33× → 31-91×) — mail-parser
itself is unchanged, the ratio grew because mailrs's constant-time
header walk got cheaper. Worst-case (50 short headers, 20-byte lines)
sees only 3% change because SIMD memchr's per-call overhead
amortises poorly on inputs near the SIMD vector width.

Run: `cargo bench -p mailrs-rfc5322 --bench parse`.

### `mailrs-mail-builder` (criterion, `cargo bench -p mailrs-mail-builder`)

3-run honest medians, v4 ckpt 24 (2026-06-03):

| Path | Before (windows-walk) | After (memchr/memmem) | Win |
|---|---:|---:|---:|
| `build/short_plain` | 1.45 µs | 1.48 µs | noise (no scan touched) |
| `build/plain_plus_html` | 4.59 µs | 3.79 µs | **−17 %** |
| `build/with_16k_attachment` | 56.7 µs | 29.0 µs | **−49 % (1.95×)** |
| `lint/short_plain` | 160 ns | 133 ns | **−17 %** |
| `lint/plain_plus_html` | 577 ns | 459 ns | **−20 %** |
| `lint/with_16k_attachment` | 7.92 µs | 6.87 µs | **−13 %** |
| `envelope/alternative_small` | 2.63 µs | 922 ns | **−65 % (2.85×)** |
| `envelope/mixed_with_16k_attachment` | 26.6 µs | 5.13 µs | **−81 % (5.19×)** |

**v4 ckpt 24** (2026-06-03): Case B — two `windows()`-style byte
walks rewritten to memchr-anchored / memmem scans:

* `strict::find_header_terminator` — the `\r\n\r\n` body-separator
  scan that runs on every `lint()` call. Was `raw.windows(4).position(|w| w == b"\r\n\r\n")`;
  is now `memchr(b'\n')` + 4-byte shape check, the same pattern shipped
  in `mailrs-arc` ckpt 10 `take_header_block`.
* `multipart::contains_subslice` — the boundary-collision scan that
  runs on every part inside `multipart_envelope`. Was
  `haystack.windows(needle.len()).any(|w| w == needle)`; is now
  `memchr::memmem::find(haystack, needle).is_some()` (Two-Way SIMD).

The collision-scan rewrite is the big-ticket win: a 16 KiB attachment
no longer walks 16 384 four-byte windows per boundary candidate,
yielding a 5.19× speed-up on the realistic mixed-MIME outbound path
(DSN / bounce reports + attached message bodies). The lint-side win
(13–20 %) is smaller because the scan is amortised across the entire
body-line-length check that follows.

Stone was Case C before — no `benches/` existed. Added
`benches/mail_builder.rs` covering the three realistic outbound
shapes (short plain, plain+html alt, mixed with 16 KiB attachment).
Run: `cargo bench -p mailrs-mail-builder --bench mail_builder`.

### `mailrs-sieve-core` (criterion, `cargo bench -p mailrs-sieve-core`)

3-run honest medians, v4 ckpt 25 (2026-06-03):

| Path | Before | After | Win |
|---|---:|---:|---:|
| `tokenize/typical` | 1.78 µs | 1.41 µs | **−21 %** |
| `tokenize/heavy` | 2.08 µs | 2.02 µs | noise |
| `compile/typical` | 2.89 µs | 2.86 µs | noise |
| `compile/heavy` | 4.25 µs | 4.04 µs | **−5 %** |
| `evaluate/typical` | 3.60 µs | 3.56 µs | noise |
| `evaluate/heavy` | 7.18 µs | 5.93 µs | **−17 %** |

**v4 ckpt 25** (2026-06-03): Case B+C — three rewrites land in this
stone, the actual engine behind the `mailrs-sieve` wrapper.

* `match_str::match_string` was the evaluator hot path's biggest
  alloc source: every `header` / `address` / `envelope` / `hasflag`
  test produced **two fresh `String`s** (`haystack.to_ascii_lowercase()`
  + `needle.to_ascii_lowercase()`) before comparing. v4 ckpt 25
  drops both allocations: `MatchType::Is` goes through
  `[u8]::eq_ignore_ascii_case`, and `:contains` / `:matches` use a
  `memchr2`-anchored case-insensitive substring search (jumps to
  each candidate via the lowercase + uppercase variant of the
  needle's first byte, then verifies with `eq_ignore_ascii_case`).
* `match_str::glob_match` (`:matches`) was a recursive
  byte-by-byte backtracker — exponential on patterns like
  `*a*b*c*`. The rewrite splits the pattern on `*`, drives each
  literal chunk through `memchr2` + `eq_ignore_ascii_case`, and
  only falls back to one-byte-at-a-time scanning when a chunk
  contains `?` (and even then, anchors on the first non-`?` byte
  via `memchr2`).
* Lexer line-comment (`# ...`) and block-comment (`/* ... */`)
  skip loops now use `memchr(b'\n')` / `memchr(b'*')` instead of
  byte-by-byte index walks.

The 21 % tokenize win is the line-comment memchr; the 17 %
evaluate-heavy win is the alloc-free match path. The
production effect is bigger than these benches show because
real inbound messages are 10-50 KiB (not the 150 B fixture) and
real Sieve scripts test more rules — every saved `String` alloc
scales with the script × header count.

Stone was Case C before — no `benches/` existed. Added
`benches/sieve_core.rs` covering tokenize / compile / evaluate
on a "typical" 3-rule script + a "heavy" 5-rule script with
multi-needle string-lists and `:matches` patterns.
Run: `cargo bench -p mailrs-sieve-core --bench sieve_core`.

### `mailrs-sieve` (criterion, `cargo bench -p mailrs-sieve`)

3-run honest medians, v4 ckpt 23 (2026-06-03):

| Path | Median | Notes |
|---|---:|---|
| `compile_sieve/typical` | **1.18 µs** | was claimed 2.1 µs — real is **1.78× faster** since the v8 ckpt 6 swap to mailrs-sieve-core 0.2 |
| `evaluate_sieve/typical` | **1.48 µs** | was claimed 3.5 µs — real is **2.36× faster** since the engine swap |

**v4 ckpt 23** (2026-06-03): Case A verified — `grep iter().position` /
`.windows(N)` / `push_str(&format!(...))` / `String::replace` in
src/ → 0 hits. The crate is a 1.7k LOC `lib.rs` whose only job
is wrapping `mailrs-sieve-core` 0.2 behind a stable
`SieveAction { Keep, FileInto, Discard, Redirect, Reject, Vacation }`
enum (the v8 ckpt 6 swap from sieve-rs to mailrs-sieve-core). All
parser / evaluator hot paths live in `mailrs-sieve-core` (handled
in ckpt 25).

The big drop (2.1 µs → 1.18 µs compile, 3.5 µs → 1.48 µs evaluate)
reflects the v8 ckpt 6 engine swap: mailrs-sieve-core is the
spec-built byte-level interpreter that replaced the AGPL `sieve-rs`
parser tree. The wrapper itself is just enum-shape mapping.

### `mailrs-attachment-extract` (criterion, `cargo bench -p mailrs-attachment-extract`)

3-run honest medians, v4 ckpt 22 (2026-06-03):

| Path | Median | Notes |
|---|---:|---|
| `extraction_method/text_plain` | **18.9 ns** | Content-Type byte-match dispatch; was claimed 27 ns — real is ~30 % faster |
| `extraction_method/application_pdf` | **24.3 ns** | same dispatch path; was claimed 45 ns — real is ~46 % faster |

**v4 ckpt 22** (2026-06-03): Case A verified — `grep iter().position` /
`.windows(N)` / `push_str(&format!(...))` / `String::replace` in
src/ → 0 hits. The crate is a single 346-line `lib.rs` whose only
job is dispatching by `Content-Type` to the right extractor crate
(`pdf-extract` / `tesseract` / `html2text` / etc.). The benched
ops are pure dispatch lookup at the criterion noise floor; actual
extraction wall-clock is delegated work and not in scope here.

### `mailrs-intelligence` (criterion, `cargo bench -p mailrs-intelligence`)

3-run honest medians, v4 ckpt 21 (2026-06-03):

| Path | Median | Notes |
|---|---:|---|
| `extract_structured_data/short_single_event` | **687 ns** | regex-free byte-scan for event / order patterns in body text |
| `extract_structured_data/long_with_flight_and_order` | **4.75 µs** | was claimed 9.3 µs — real is **1.96× faster** than the candidates-table number (cumulative rustc / regex / pattern improvements) |
| `calculate_importance` | **2.93 ns** | was claimed 7.4 ns — at the criterion noise floor; integer-only score combination |

**v4 ckpt 21** (2026-06-03): Case A verified — `grep iter().position` /
`.windows(N)` / `push_str(&format!(...))` / `String::replace` in
src/ → 0 hits. The crate is 2.3k LOC across 7 files; the
prod-relevant code paths are LLM-bound (`openai_compatible.rs`
client + `provider.rs` trait) and the non-LLM heuristic extractors
(`structured.rs` byte-pattern scans for flight numbers / order
IDs / dates / `calculate_importance` integer score combo). The
extractors are at the sub-µs / single-digit-ns range — fine on a
per-message hot path. LLM call latency dominates total wall-clock
when AI scoring is enabled.

`extract_structured_data/long_with_flight_and_order` drifted from
9.3 µs claim to 4.75 µs real — a 1.96× improvement on the same
binary. Likely a rustc-driven regex / pattern improvement (the
underlying regex / `regex_lite` paths have benefited from work
landed in the 6 weeks since the original measurement).

### `mailrs-postmaster` (criterion, `cargo bench -p mailrs-postmaster`)

3-run honest medians, v4 ckpt 20 (2026-06-03):

| Path | Median | Notes |
|---|---:|---|
| `extract_bimi_logo_url` | **40 ns** | BIMI TXT-record URL extraction; was claimed 44 ns — slight under-claim, real is ~10 % faster |

**v4 ckpt 20** (2026-06-03): Case A verified — `grep iter().position` /
`.windows(N)` / `push_str(&format!(...))` / `String::replace` in
src/ → 0 hits. The crate is 2.3k LOC across 11 files but ~95 % of
that is async DNS-bound diagnostic checks (mx / dkim / dmarc / dane /
mta_sts / ptr / spf / tlsrpt / bimi / resolver) — production
wall-clock is dominated by DNS round-trips (5-50 ms), not the CPU
portion of any check. The single benched op (`extract_bimi_logo_url`)
is a 40 ns regex-free string extraction at the criterion noise
floor.

### `mailrs-clean` (criterion, `cargo bench -p mailrs-clean`)

Re-measured v4 ckpt 19 (2026-06-03), 3-run honest medians:

| Path | Median | Notes |
|---|---:|---|
| `clean_email_html/short_60b` | **10.5 µs** | constant-overhead floor (html2text setup) |
| `clean_email_html/marketing_500b` | **30 µs** | small marketing — was claimed 28 µs, drift +7 % (noise) |
| `clean_email_html/marketing_5kb` | **144 µs** | was claimed 188 µs; real number is 23 % faster — cumulative rustc / html2text improvements since v4 round 6 measurement |
| `clean_email_html/marketing_50kb` | **1.67 ms** | was claimed 2.42 ms; real is 31 % faster — same drift direction, large-input gains tracked memcpy-floor improvements |
| `sender_heuristics/detect_bulk_sender_yes` | **27 ns** | regex-free byte scan |
| `sender_heuristics/is_automated_sender_yes` | **31 ns** | same path |
| `split_quoted_content` | **285 ns** | quote-line walk |

**v4 ckpt 19** (2026-06-03): Case A verified — `grep iter().position`
/ `.windows(N)` / `push_str(&format!(...))` / `String::replace` in
src/ → 0 hits. The crate is 1k LOC across 4 files; `html.rs` has
11 `memchr::memmem::find_iter` / `memchr::memchr` call sites
already (single-tag fast-paths + boundary scans). The v4 round 6
win (fused 5 single-tag scans + killed the quadratic comment
loop) is still load-bearing.

The "previous claim" column shifted slightly faster on 3-run honest
re-measure — likely a mix of rustc / `html2text` minor releases
and noise across the 6 weeks since the prior measurement. No
over-claim retract needed (faster is fine).

### Server-internal (`mailrs-server`, gated `#[test]` bench)

| Path | Measurement | Run command |
|---|---|---|
| `extract_subject_and_from` vs. two `extract_header` calls | Single-pass wins **48-50%** across 1KB/5KB/20KB messages (release). Absolute: saves **2.0 / 3.1 / 6.5 µs** per message respectively. | `MAILRS_BENCH=1 cargo test --release -p mailrs-server bench_two_pass_vs_single_pass -- --nocapture --test-threads=1` |

### Frontend (`web/`, vite production build, gzip via `gzip -c | wc -c`)

The headline number for the web frontend is **first-paint JS cost on the
authenticated mail path** — i.e. the bytes the browser must download and
parse before the conversation list can render. The mail path is the hot
path; everyone landing on `/mail/...` hits it on every cold cache.

| Path | Cold-load JS (gzip) | Reproduce |
|---|---:|---|
| `/login` (entry chunk only) | **159.85 kB** | `cd web && bun run build && gzip -c dist/assets/index-*.js \| wc -c` |
| `/mail/...` (entry + chat shell) | **219.98 kB** | entry + `chat-*.js` only — markdown/tiptap libs are now lazy |
| `/admin/overview` (entry + admin shell + overview tab) | **~164 kB** | entry + `admin-*.js` + `admin-overview-*.js` |
| Inbox HTML-or-markdown email opened (entry + chat + markdown viewer chunk + lib chunks) | **~318 kB** | only paid when the user actually opens an email that requires markdown rendering |
| Compose form opened with active signature (adds `signature-block-*` + `rich-editor-*` on top) | **~452 kB** | only paid when the user opens compose with signatures enabled |

Compare to pre-polish baseline (2026-05-22, before any of this commit):

| Path | Before (gzip) | After (gzip) | Δ |
|---|---:|---:|---:|
| `/login` cold | 159.78 kB | 159.85 kB | +0.04 % |
| `/mail/...` cold | **450.99 kB** | **219.98 kB** | **−51.2 %** |
| `/admin/...` cold (overview) | ~174 kB (one 14.48 kB chunk forces all 11 admin tabs) | ~164 kB (per-tab split) | −5.8 % to first tab |
| Total `dist/` (incl. fonts) | 5.2 MB | 5.3 MB | +1.9 % (more chunks → more URL overhead, fonts unchanged) |
| JS chunk count | 16 | 35 | +119 % (better caching granularity) |

The headline win — `/mail/...` cold-load down **51.2 %** (450.99 → 219.98 kB
gzip) — comes from one specific change: react-markdown + remark-gfm +
rehype-highlight + lowlight + highlight.js + tiptap + prosemirror all used to
ship inside the chat chunk because `MessageBubble` / `StructuredCompose` /
`SignatureBlock` / `TextBlock-preview` imported them eagerly. After splitting:

- `MessageBubble` → only renders markdown when `looksLikeMarkdown(body)` matches;
  the markdown pipeline ships as `markdown-viewer-*.js` (0.65 kB gzip) +
  `lib-*-{47.40,51.38}.js` chunks (highlight.js + react-markdown internals).
  Plain-text emails skip them entirely.
- `TextBlock` preview tab → lazy `markdown-preview-*.js`.
- `SignatureBlock` → lazy `signature-block-*.js` + `rich-editor-*.js`
  (131.75 kB gzip — tiptap + prosemirror + lowlight + highlight.js
  language pack). Only fetched when a compose surface mounts with a
  signature configured.
- Admin sub-pages → each is its own chunk (1.5–3.4 kB gzip per tab).
  Previously all 11 shipped as one ~14.5 kB chunk.

Run the baseline-and-after measurement yourself:

```bash
cd web
git checkout <pre-polish-commit> -- src/
bun install --frozen-lockfile
bun run build 2>&1 | grep -E '^dist/assets/(index|chat|admin)' | sort -k3 -h
gzip -c dist/assets/index-*.js dist/assets/chat-*.js | wc -c   # = pre-polish

git checkout <perf-polish-commit> -- src/
bun install --frozen-lockfile
bun run build 2>&1 | grep -E '^dist/assets/(index|chat|admin)' | sort -k3 -h
gzip -c dist/assets/index-*.js dist/assets/chat-*.js | wc -c   # = after
```

(Pre-polish + after totals shown in the table are from the same tool — `gzip
-c | wc -c` on the chunks rolldown emitted to `dist/assets/` for each variant.)

#### What we did NOT do (and why)

- **No `manualChunks` config.** A previous attempt (per the comment in
  `vite.config.ts:49-51`) split tiptap and markdown into manual groups and
  ended up dragging jotai with them, which then leaked back into the entry
  chunk's preload. Rolldown's automatic chunking respecting dynamic imports
  is good enough once the lazy boundaries are in the source code.
- **No virtualization-of-thread-view.** Thread view caps visible messages at
  3 by default (with "show earlier" button to expand). Adding `react-window`
  would add ~5 kB of code for zero measurable wins on the realistic message
  counts.
- **No `lucide-react` icon refactor.** All 29 import sites already use
  named imports — tree-shaking works, only the icons actually used ship.
- **No font subsetting.** RobotoFlex.ttf is 1.79 MB but loaded by the gds
  design system as part of `useFonts()`. Subsetting requires gds work.
- **No service-worker prefetch of lazy chunks.** Could be a follow-up:
  `sw.js` could prefetch `chat-*.js` after the entry settles, so users
  arriving on /login pay zero waiting cost when they then click into mail.

#### Frontend perf gate (no number quotation outside the table)

Same rule as the Rust crates: every kB and percent in this section must
trace back to `bun run build` output captured on a clean checkout. New
chunks must be added to the table when they change the headline metric.
Lighthouse / WebPageTest runs are welcome but their numbers do not enter
this table unless they are run on a fresh deploy with a documented
environment (cold cache, throttled to 4G, etc.).

#### Verification: tests, type-check, lint

- `bun run test` — **451 passed** (25 files), 2.4–3.5 s (unchanged from baseline)
- `bun run check` (tsc + eslint + prettier) — clean
- `bun run build` — completes in ~300-1500 ms after type-check / lint / format
- All chunks are content-hashed; the service worker (`public/sw.js`) caches
  the shell only; lazy chunks are fetched on demand by the browser.

### Variance note

All numbers above are **criterion 100-sample median on a single M-series
Mac running release profile**. Re-running on the same machine within
minutes typically lands within ±5% of these medians; under heavy
concurrent load (a build going at the same time) sub-µs-scale benches
can swing ±30%. Order-of-magnitude is stable; sub-nanosecond comparisons
between two paths should always be re-measured on the consumer's own
hardware.

### Surfaced potential perf candidates

1. `mailbox::InMemoryMailboxStore::query_messages` is ~120 µs for 1k
   messages because the fixture clones every matching `Message`
   (12+ String fields each). The PG impl pushes the work into SQL.
   Acceptable as fixture; flagged in README §"Performance".
2. **`inbound::make_delivery_decision(Junk)` — partially fixed.** Was
   ~735 ns vs ~337 ns Accept (2.4× gap). Replaced the `format!` macro
   + `matched_rules.join(", ")` with a single pre-sized `String` +
   `write!` macro + inline join: now measures **671 ns** (`cargo bench
   -p mailrs-inbound -- decision/make_delivery_decision_junk`,
   M-series Mac, release). **-64 ns / -8.7%** real measured. The gap
   to Accept narrowed; remaining cost is `build_auth_header` which
   both paths share.
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

### v6 ckpt 3 — P2 crates measured (criterion `--quick`, busy laptop)

Quick-mode (10 samples) ballpark, run during the v6 ckpt 3 polish
pass to confirm every P2 crate has a criterion bench producing
numbers. Use the per-crate sections above for the higher-confidence
medians; these are regression-catch ballpark.

| Crate | Bench | Median (`--quick`) |
|---|---|---:|
| `mailrs-outbound-queue` | `dkim_sign/short` | **288 µs** (was 2.27 ms pre-v1.7.35; aws-lc-rs swap) |
| `mailrs-outbound-queue` | `dkim_sign/long_8kb` | **309 µs** (was 2.71 ms pre-v1.7.35; aws-lc-rs swap) |
| `mailrs-outbound-queue` | `retry_delay_secs` (×10) | 3.4 ns |
| `mailrs-outbound-queue` | `should_bounce` (×10) | 3.3 ns |
| `mailrs-shield` | `greylist/evaluate_retry` | 2.2 ns |
| `mailrs-shield` | `greylist/triplet_key` | 50 ns |
| `mailrs-shield` | `ptr_score_from_names(match)` | 135 ns |
| `mailrs-shield` | `ptr_score_from_names(no_match)` | 410 ns |
| `mailrs-clean` | `clean_email_html/short_60b` | 18 µs |
| `mailrs-clean` | `clean_email_html/marketing_500b` | 56 µs |
| `mailrs-clean` | `clean_email_html/marketing_5kb` | 315 µs |
| `mailrs-clean` | `clean_email_html/marketing_50kb` | 2.85 ms |
| `mailrs-clean` | `sender_heuristics/detect_bulk_sender_yes` | 42 ns |
| `mailrs-clean` | `sender_heuristics/is_automated_sender_yes` | 57 ns |
| `mailrs-clean` | `sender_heuristics/is_automated_sender_no` | 54 ns |
| `mailrs-clean` | `split_quoted_content` | 526 ns |
| `mailrs-postmaster` | `extract_bimi_logo_url` | 44 ns |
| `mailrs-intelligence` | `extract_structured_data/short_single_event` | 709 ns |
| `mailrs-intelligence` | `extract_structured_data/long_with_flight_and_order` | 9.3 µs |
| `mailrs-intelligence` | `calculate_importance` | 7.4 ns |
| `mailrs-attachment-extract` | `extraction_method/text_plain` | 27 ns |
| `mailrs-attachment-extract` | `extraction_method/application_pdf` | 45 ns |
| `mailrs-sieve` | `compile_sieve/typical` | 2.1 µs |
| `mailrs-sieve` | `evaluate_sieve/typical` | 3.5 µs |

**Findings during the measurement pass:**

- `mailrs-outbound-queue::dkim_sign` was ~3-4× slower than the
  pre-v1.7.31 mail-auth baseline. Two causes, both closed:
  1. `DkimSignConfig::sign` was parsing the PKCS#8 PEM into an
     `RsaPrivateKey` on every call — fixed in commit `172dde2`
     (v1.7.32) with an `OnceLock` cache shared across worker clones.
  2. The `rsa` crate's RSA-2048 PKCS#1 v1.5 sign primitive itself
     was the dominant residual (~1.5 ms / sign) vs `mail-auth`'s
     default `aws-lc-rs` backend (~0.5 ms). Swapped in v1.7.35
     (commit `fca3c12`) — `mailrs-dkim` 3.0 now wraps
     `aws_lc_rs::signature::RsaKeyPair`, taking sign per-call from
     2.27 ms / 2.71 ms (short / long_8kb) down to 288 µs / 309 µs
     measured. **8-9× speed-up**, full parity with mail-auth's
     pre-cutover throughput.
- All other P2 benches are within reasonable ballpark for their
  crate size; no further hot-path investigation triggered.

Run:

```bash
for c in outbound-queue shield clean postmaster intelligence \
         attachment-extract sieve; do
  bn=$(ls crates/$c/benches/ | head -1 | sed 's/\.rs$//')
  cargo bench -p mailrs-$c --bench $bn -- --quick
done
```

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
