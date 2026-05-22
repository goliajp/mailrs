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
| SMTP receive throughput (perf-first vs vanilla profile) | **+2.10%** throughput (267.2 vs 261.7 msg/s median, 3 rounds × 30s × 32 conns); **p99 latency −5.57%** (179.7 ms vs 190.3 ms). The original commit claim of "+10-20% throughput" was wrong; the real measured win is much smaller but still positive and consistent. Binary-size win is the dominant payoff of the perf-first profile. | `scripts/bench-smtp-load.sh 30 32 3` (builds both `release` and `release-vanilla` profiles, runs 3 rounds each, prints comparison) |

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
| `decision::make_delivery_decision_junk` | **671 ns** | Junk path — was 735 ns; commit `b8ea44d` replaced `format!` + `matched_rules.join` with pre-sized `String` + `write!` for **−8.7%** measured |

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
| `format_ehlo_response` | **35 ns** | was 307 ns; commit `19aa482` replaced `write!`-macro dispatch with direct `push_str` of `&str` segments for **−89%** measured (~9× faster) |
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
| `triplet_key` | **~25 ns** | was 120 ns; commit `d0c5941` replaced `format!` with pre-sized `String::with_capacity` + `push_str` for **−82%** measured (~5× faster). Called per inbound message on the greylist hot path. |

### `mailrs-clean` (criterion, `cargo bench -p mailrs-clean`)

| Path | Median | Notes |
|---|---:|---|
| `clean_email_html(5 KB marketing)` | **~336 µs** | typical-size hot path |
| `clean_email_html(50 KB worst-case)` | **~2.5 ms** | **~22 MB/s** throughput |

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
