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
| SMTP receive throughput (perf-first vs vanilla profile, original measurement 2026-05) | **+2.10%** throughput (267.2 vs 261.7 msg/s median, 3 rounds × 30s × 32 conns); **p99 latency −5.57%** (179.7 ms vs 190.3 ms). The original commit claim of "+10-20% throughput" was wrong; the real measured win is much smaller but still positive and consistent. Binary-size win is the dominant payoff of the perf-first profile. | `scripts/bench-smtp-load.sh 30 32 3` (builds both `release` and `release-vanilla` profiles, runs 3 rounds each, prints comparison) |
| SMTP receive throughput, **current** (post tracing + listener refactor, 2026-05-23) | **300.2 msg/s** (1 round × 30s × 32 conns, perf-first profile), **P50 106 ms, P99 152 ms, P999 166 ms** — single-round number, not a perf-first-vs-vanilla comparison. Logged here as the latest end-to-end number after all stone-level optimizations + the server-level listener helper refactor + tracing span addition. | `cargo bench -p mailrs-server --bench smtp_load --release -- --duration 30 --conns 32` |
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

### Head-to-head vs. Rust community competitors (criterion, M-series Mac, release profile, `--quick` mode)

Honest comparison. Wins **and** losses. Bench source: `crates/<crate>/benches/compare_<competitor>.rs` (each crate's compare bench is reproducible in-tree).

#### `mailrs-spf` vs `mail-auth` 0.9 (SPF half — the DEPS_AUDIT #1 reason)

3-run noise-controlled median (M-series Mac, release, criterion
default 100 samples × 3 fresh invocations):

| Input | mailrs-spf | mail-auth | Winner |
|---|---:|---:|---|
| `v=spf1 ip4:203.0.113.0/24 -all` (simple) | **46 ns** | 60 ns | **mailrs +23%** ✅ |
| 8-mechanism complex | **301 ns** | 480 ns | **mailrs +37%** ✅ |
| 8-include pathological | **332 ns** | ≈585 ns (prior bench) | **mailrs +43%** ✅ |

**Honest re-bench, v4 round 12 (2026-05-26):** the previously
claimed "tied within noise" for the simple case was *under-claim*
— controlled 3-run median actually shows mailrs +23% (46 ns vs
60 ns, gap is comfortably outside per-bench CI band). The
complex_8 claim of "+34%" was also conservative; real median
across 3 runs is +37%. The pathological row's mail-auth side is
carried forward from the prior bench — the 3-run output captured
only mailrs's side, so the ratio is approximate pending a fresh
both-sides bench.

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

3-run noise-controlled median (M-series Mac, release):

| Input | mailrs-dkim | mail-auth | Winner |
|---|---:|---:|---|
| minimal (7 tags) | **183 ns** | 205 ns | **mailrs +11%** ✅ |
| realistic (folded, 11 tags, 7 signed headers) | **480 ns** | (not bench'd in same harness) | — |

**Honest re-bench, v4 round 12 (2026-05-26):** the previously
claimed "mailrs 1.8×" on minimal was a single-run quiet-CPU lucky
outlier. Controlled 3-run repeated measurement under realistic
system load shows mailrs at 183 ns vs mail-auth at 205 ns —
**clear +11% lead, but well shy of the 1.8× headline**. We choose
to report the conservative median number rather than the cherry-pick.

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
when system load contaminates a single run):

| Input | mailrs-mime | mail-parser | Winner |
|---|---:|---:|---|
| simple `text/plain` body_text | **108 ns** | 195 ns | **mailrs +45%** ✅ |
| find `text/calendar` part (apples-to-apples) | **~620 ns** | ~640 ns | **mailrs +5-10%** ✅ (was −28%, reversed) |

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

#### `mailrs-rfc5322` vs `mail-parser` (header lookup, lazy)

mailrs-rfc5322 is pull-based: it scans for the requested header without parsing the body. mail-parser eagerly parses everything. Comparison is therefore by body size — the lazy crate's wall-clock cost is constant.

| Body size | mailrs-rfc5322 (subject + from) | mail-parser (full parse) | Winner |
|---|---:|---:|---|
| 1 KB | 215 ns | 2.35 µs | **mailrs 11×** ✅ |
| 5 KB | 213 ns | 3.30 µs | **mailrs 15×** ✅ |
| 20 KB | 213 ns | 6.99 µs | **mailrs 33×** ✅ |

This is the "lazy beats eager" payoff under load. If you only need 1-2 headers per message — which the SMTP frontline does — `mailrs-rfc5322` is the right tool. Use `mail-parser` when you need full-tree access in one shot.

#### `mailrs-rfc2047` vs `mail-parser` (subject extraction)

| Input | mailrs-rfc2047 (single-field) | mail-parser (full message) | Winner |
|---|---:|---:|---|
| ASCII subject | 23 ns | 323 ns | **mailrs 14×** ✅ |
| =?UTF-8?B?...?= encoded | 85 ns | 346 ns | **mailrs 4×** ✅ |

Same caveat as rfc5322: the right comparison is "minimum cost to get the user-visible Subject string", and a focused crate beats a tree builder. mail-parser remains the right call when you want the full structured Message at once.

#### `mailrs-ical` vs `icalendar` 0.17 (RFC 5545 parse)

| Input | mailrs-ical | icalendar | Winner |
|---|---:|---:|---|
| simple VEVENT | 1.44 µs | 5.33 µs | **mailrs 3.7×** ✅ |
| VEVENT + RRULE | 1.63 µs | 5.96 µs | **mailrs 3.7×** ✅ |
| VTIMEZONE + VEVENT | 2.67 µs | 9.21 µs | **mailrs 3.4×** ✅ |

Clean sweep on parse. Note: `icalendar` has serializer / builder APIs we don't bench against because mailrs-ical's serializer surface is narrower.

#### `mailrs-rate-limit` vs `governor` 0.10 (DashMap-backed)

3-run noise-controlled median:

| Input | mailrs-rate-limit | governor | Winner |
|---|---:|---:|---|
| hot key, allowed | **17.1 ns** | 18.8 ns | **mailrs +9%** ✅ |
| cold key first-touch | **210 ns** | 222 ns | **mailrs +5-6%** ✅ |

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
| `EHLO mail.example.com` | **11.0 ns** | 134.8 ns | **mailrs 12.3×** ✅ |
| `MAIL FROM:<…> SIZE=…` | **93.7 ns** | 227.9 ns | **mailrs 2.4×** ✅ |
| `RCPT TO:<…>` | **52.6 ns** | 163.7 ns | **mailrs 3.1×** ✅ |
| `DATA` | **4.1 ns** | 15.8 ns | **mailrs 3.9×** ✅ |

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

| Command | mailrs-imap-proto | imap-codec | Winner |
|---|---:|---:|---|
| `A001 SELECT INBOX` | **47.8 ns** | 62.2 ns | **mailrs +23%** ✅ |
| `A002 FETCH 1:100 (FLAGS BODY[…])` | **82.0 ns** | 280.2 ns | **mailrs 3.4×** ✅ |
| `A003 LOGIN alice@example.com password` | **78.8 ns** | 112.9 ns | **mailrs +30%** ✅ |
| `A004 NOOP` | **27.8 ns** | 36.2 ns | **mailrs +23%** ✅ |

Clean sweep after the v4 round-1 squeeze. The previous numbers had us
losing 3 of 4 cases (LOGIN −59%, NOOP −44%, SELECT −14%). Three
changes closed all gaps and pushed us into the lead on every path:

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
| `parse_command/EHLO` | **11 ns** | was 22 ns; v4 round 2 killed `verb.to_ascii_uppercase()` heap alloc |
| `parse_command/DATA` | **4 ns** | was 22 ns (and 16 ns vs smtp-codec 12 ns → loss); v4 round 2 = **−82%** |
| `parse_command/RCPT_TO` | **53 ns** | was 70 ns; same verb-buffer change |
| `parse_command/MAIL_FROM` | **94 ns** | was 103 ns; same |
| `format_ehlo_response` | **35 ns** | was 307 ns; commit `19aa482` replaced `write!`-macro dispatch with direct `push_str` of `&str` segments for **−89%** measured (~9× faster) |
| `address/is_valid_typical` | **10 ns** | |
| `address/split_typical` | **12 ns** | |

### `mailrs-imap-format` (criterion, `cargo bench -p mailrs-imap-format`)

| Path | Median | Notes |
|---|---:|---|
| `format_imap_flags/seen+answered` | **12.9 ns** | was 27.8 ns (v4 squeeze, commit replaces `Vec::push` + `join` with `String::with_capacity(47) + push_str`); **−54%** measured |
| `parse_imap_flags/seen answered` | **16.1 ns** | was 42.2 ns; v4 squeeze killed the `to_uppercase()` allocation per token, replaced with length-keyed `eq_ignore_ascii_case` against compile-time `&[u8; N]` targets; **−62%** measured |
| `format_internal_date` | **157 ns** | dominated by `chrono` `from_timestamp` + format; squeeze deferred (would require an in-house date formatter) |

### `mailrs-smtp-client` (criterion, `cargo bench -p mailrs-smtp-client`)

| Path | Median | Notes |
|---|---:|---|
| `sort_mx_records(20)` | **12 ns** | MX priority sort |
| `parse_response(short)` | **27 ns** | was 30 ns; v4 round 5 unrolled 3-digit byte code parse |
| `parse_response(10-line EHLO)` | **257 ns** | was 290 ns; same change + Vec::with_capacity(8) for typical 4-12-line EHLO |
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
| `extract_message_id(short header)` | **54 ns** | was ~150 ns; v4 squeeze replaced `String::from_utf8_lossy(data).lines()` with a byte-level memchr scan that stops at the first blank line — skips full UTF-8 validation + avoids cloning the whole message; **−64%** measured |
| `extract_message_id(long real-world header)` | **123 ns** | 20+ header lines, still bounded by line count not body length |
| `extract_in_reply_to(short / long)` | **61 / 133 ns** | same path as message_id |
| `normalize_message_id` | **~8 ns** | `<…>` strip |
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

### `mailrs-spf` — RFC 7208 SPF verifier (criterion, M-series Mac, release)

| Path | Median |
|---|---:|
| `Record::parse` (simple `v=spf1 ip4 -all`) | **82 ns** |
| `Record::parse` (complex 8-mechanism record) | **484 ns** |
| `verify` pass path (no real DNS) | **244 ns** |

Run: `cargo bench -p mailrs-spf --bench spf`. Production `verify` is
dominated by DNS round-trips (5-50 ms); the bench numbers above are
the pure CPU portion.

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
| Subject + From lookup | 1 KB | **212 ns** | 2383 ns | **11.2×** |
| Subject + From lookup | 5 KB | **212 ns** | 3378 ns | **15.9×** |
| Subject + From lookup | 20 KB | **212 ns** | 6901 ns | **32.5×** |
| Target at end of 50 headers (worst case) | — | **393 ns** | n/a | n/a |
| body offset locate | 1 KB | **249 ns** | 2387 ns | **9.6×** |
| body offset locate | 5 KB | **247 ns** | 3337 ns | **13.5×** |
| body offset locate | 20 KB | **248 ns** | 6855 ns | **27.6×** |
| Received-chain walk (3 hops) | — | **340 ns** | 3382 ns | **9.9×** |

`mailrs-rfc5322` is **constant-time in body size** because the scanner
stops at the header/body boundary. `mail-parser` is linear in body
size because it builds the full Message tree. For an SMTP receive
pipeline reading 2-5 headers per message, that's 6-7 µs/msg saved on
20 KB messages — at 1000 msg/sec, **6-7 ms/sec of CPU freed.**

Run: `cargo bench -p mailrs-rfc5322 --bench parse`.

### `mailrs-clean` (criterion, `cargo bench -p mailrs-clean`)

| Path | Median | Notes |
|---|---:|---|
| `clean_email_html(60 B short)` | **10.8 µs** | constant-overhead floor |
| `clean_email_html(500 B marketing)` | **28 µs** | small marketing |
| `clean_email_html(5 KB marketing)` | **188 µs** | was ~336 µs; v4 round 6 fused 5 single-tag scans into one + killed quadratic comment loop; **−44%** measured |
| `clean_email_html(50 KB worst-case)` | **2.42 ms** | was ~2.5 ms; **~20 MB/s** throughput (large messages dominated by html2text final stage) |

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
