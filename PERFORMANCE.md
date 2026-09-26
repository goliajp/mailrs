# Performance

Published numbers for mailrs and its library crates, each with the
command that reproduces it. Unless a section says otherwise, numbers are
criterion medians on an M-series Mac with the release profile; see the
variance note at the end before comparing across machines.

Every crate with a bench runs it the same way:

```bash
cargo bench -p mailrs-<crate>
```

### Workspace-level

| Path | Measurement | Run command |
|---|---|---|
| Release binary size (mailrs-server) | 44 MB (default) → 22 MB (perf-first profile). M-series Mac. | `du -h $TARGET_DIR/release/mailrs-server` before/after commit `9f21e0b`. |
| SMTP receive throughput (perf-first vs vanilla profile, original measurement 2026-05) | **+2.10%** throughput (267.2 vs 261.7 msg/s median, 3 rounds × 30s × 32 conns); **p99 latency −5.57%** (179.7 ms vs 190.3 ms). Binary size is the main payoff of the perf-first profile. | `scripts/bench-smtp-load.sh 30 32 3` (builds both `release` and `release-vanilla` profiles, runs 3 rounds each, prints comparison) |
| SMTP receive throughput, **current** (post tracing + listener refactor, 2026-05-23) | **300.2 msg/s** (1 round × 30s × 32 conns, perf-first profile), **P50 106 ms, P99 152 ms, P999 166 ms** — single-round number, not a perf-first-vs-vanilla comparison. Logged here as the latest end-to-end number after all crate-level optimizations + the server-level listener helper refactor + tracing span addition. | `cargo bench -p mailrs-server --bench smtp_load --release -- --duration 30 --conns 32` |
| SMTP receive throughput, **post DeliveryExecutor** (`mailrs-delivery-executor` 1.0 group-commit, 2026-05-24) | **999 msg/s mean across 3 × 30s × 32 conns** (rounds: 1045 / 972 / 979). **3.4×** vs the immediately-prior 291 msg/s baseline (same hardware, same bench). **P50 32 ms** (vs 105 ms baseline = **3.3× faster**), **P99 41 ms** (vs 163 ms = **4.0× faster**), **P999 76 ms** (vs 199 ms = **2.6× faster**). All four UX axes — throughput, p50, p99, p999 — improve simultaneously; no axis regresses. The win comes from group-commit: 32 concurrent SMTP sessions delivering to the same Maildir path now share a single fsync per batch (max_batch=64, max_wait=10ms) via `mailrs-delivery-executor`'s mpsc → `Maildir::deliver_batch` pipeline, instead of each session driving its own per-message fsync. | `cargo build --profile release-debug -p mailrs-server --bench smtp_load && $CARGO_TARGET_DIR/release-debug/deps/smtp_load-* --duration 30 --conns 32 --warmup 5` |
| SMTP receive throughput, **post pipelined DeliveryExecutor** (`mailrs-delivery-executor` 1.1, max_concurrent_flushes=2, 2026-05-24) | **1079 msg/s mean across 3 × 30s × 32 conns** (rounds: 1074 / 1073 / 1089). **+8%** vs the 1.0 serial-flush 999 msg/s. **P50 29 ms** (-9%), **P99 36 ms** (-12%), **P999 45 ms (-41%)** — tail latency is the headline win. Mechanism: while batch A's fsync is in flight on a `spawn_blocking` thread, batch B starts collecting concurrently; a `Semaphore`-bounded pipeline of 2 in-flight flushes hides disk-wait behind batch-collection latency without queuing unbounded fsyncs. Cumulative since the perf-axis kickoff (#127): **291 → 1079 msg/s = 3.71× throughput**, **P999 199 → 45 ms = 4.4× faster tail**. | Same reproduce command as the 1.0 row above; binary uses the new published `mailrs-delivery-executor` 1.1 default tuning. |

### `mailrs-inbound` (criterion bench, M-series Mac, release, 100-sample median ± 95% CI from criterion's own analysis)

| Path | Median | Notes |
|---|---:|---|
| `decision::make_delivery_decision_greylist` | **2.4 ns** | trivial early return |
| `auth_header::build_auth_header_no_reason` | **30 ns** | was 342 ns; direct String builder bypasses the `Vec<AuthResult>` + `format!` chain; **−91%** / 11× ✅ |
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

### Cross-ecosystem competitor map (C / C++ / Go / Python / Zig)

Per-crate competitor audit across 5 ecosystems (Rust competitors are
already covered in the head-to-head tables below). All entries verified
2026-05-26 via GitHub / PyPI / pkg.go.dev / zigistry.dev. This snapshot
covers the 41 crates published as of 2026-05-25; `mailrs-mail-builder`
and `mailrs-sieve-core` were added afterward and are not yet
cross-language audited. "—" means
no widely-used library found; "(monolith)" means the functionality
exists only inside a full MTA/server, not as a consumable library.

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

Sources verified 2026-05-26 against GitHub, PyPI, pkg.go.dev,
zigistry.dev, and project websites.

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

The `mime` per-call cost (1 523 B / 14 allocs) is the parse-tree
weight: `ContentType.{type_, subtype}` and `Disposition.kind`
all inline into their structs (≤24 bytes ⇒ no heap), so the only
allocs that remain are the `Cow::Owned` `body` for transfer-encoded
parts plus the small `ContentType.params` HashMap nodes.
Zero leaks across 140 000 allocations confirms the recursive Walker
+ Cow tree shape drops cleanly on teardown.

The `spf` per-call cost (694 B / 6.3 allocs) is mostly the
`Mechanism::*` Vec growth (4-slot pre-sized in `Vec::with_capacity(4)`)
plus the boxed include-domain Strings. The peak (616 B / 9 blocks) is
the largest single record (`pathological_8` with 8 include strings)
alive at one moment — under 1 KB per record.

These are the two most-exercised crates (`mime` runs on every
inbound message, `spf` runs on every accepted MAIL FROM).

### Test coverage — `cargo llvm-cov --workspace`

Workspace total (line-coverage, `cargo llvm-cov --workspace --summary-only`):
**63.67 % region / 67.47 % function / 58.66 % line** (2026-05-26).

The headline number is dragged down by `mailrs-server`'s web/admin/OIDC/RSVP
handlers — framework wiring that is deliberately not unit-tested.
Published crates look very different — sampled
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
framework wiring. The workspace 80 % bar is satisfied for
all 41 published crates individually, even though the workspace-wide rollup
sits at 58.66 % because of the server binary.

Reproduce: `cargo llvm-cov --workspace --tests --summary-only --ignore-run-fail`
(perf_gate tests fail under coverage instrumentation due to inflated
budgets; `--ignore-run-fail` lets the summary still print).

### Head-to-head vs. Rust community competitors (criterion, M-series Mac, release profile, `--quick` mode)

Honest comparison. Wins **and** losses. Bench source: `crates/<crate>/benches/compare_<competitor>.rs` (each crate's compare bench is reproducible in-tree).

#### `mailrs-spf` vs `mail-auth` 0.9 (SPF half)

3-run noise-controlled median (M-series Mac, release, criterion
default 100 samples × 3 fresh invocations):

| Input | mailrs-spf | mail-auth | Winner |
|---|---:|---:|---|
| `v=spf1 ip4:203.0.113.0/24 -all` (simple) | **43 ns** | 53 ns | **mailrs +23%** ✅ |
| 8-mechanism complex | **240 ns** | 440 ns | **mailrs +45%** ✅ |
| 8-include pathological | **223 ns** | 583 ns | **mailrs +62%** ✅ |

#### `mailrs-dkim` vs `mail-auth` 0.9 (DKIM-Signature header parse)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Input | mailrs-dkim | mail-auth | Winner |
|---|---:|---:|---|
| minimal (7 tags) | **147 ns** | 175 ns | **mailrs +19%** ✅ |
| realistic (folded, 11 tags, 7 signed headers) | **448 ns** | 433 ns | **mail-auth +3%** (TIE) |

#### `mailrs-mime` vs `mail-parser` (MIME body parse)

3-run noise-controlled median (criterion default 100-sample,
each run a fresh `cargo bench` invocation; CI bands rejected
when system load contaminates a single run). Measured 2026-06-02:

| Input | mailrs-mime | mail-parser | Winner |
|---|---:|---:|---|
| simple `text/plain` body_text | **86 ns** | 210 ns | **mailrs +59%** ✅ |
| find `text/calendar` part (apples-to-apples) | **619 ns** | 664 ns | **mailrs +7%** ✅ |

The find-calendar comparison is true apples-to-apples — both sides
parse the message and walk parts looking for the `text/calendar`
mime-type, returning the body's length. Bench source:
`crates/mime/benches/mime.rs::bench_vs_mail_parser_invite`.

#### `mailrs-rfc5322` vs `mail-parser` (header lookup, lazy)

mailrs-rfc5322 is pull-based: it scans for the requested header without parsing the body. mail-parser eagerly parses everything. Comparison is therefore by body size — the lazy crate's wall-clock cost is constant.

| Body size | mailrs-rfc5322 (subject + from) | mail-parser (full parse) | Winner |
|---|---:|---:|---|
| 1 KB | **83 ns** | 2.63 µs | **mailrs 32×** ✅ |
| 5 KB | **84 ns** | 3.73 µs | **mailrs 44×** ✅ |
| 20 KB | **84 ns** | 7.68 µs | **mailrs 91×** ✅ |

This is the "lazy beats eager" payoff under load. If you only need 1-2 headers per message — which the SMTP frontline does — `mailrs-rfc5322` is the right tool. Use `mail-parser` when you need full-tree access in one shot.

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

### `mailrs-dav` — CalDAV / CardDAV (criterion, M-series Mac, release)

| Path | Median |
|---|---:|
| `etag_of` | **~54 ns** |
| `xml_escape_plain` | **~95 ns** |
| `extract_multiget_uids_3` | **~343 ns** |
| `multistatus_wrap_small` | **~135 ns** |
| `multistatus_wrap_med_20` | **~2.9 µs** |

#### `mailrs-rate-limit` vs `governor` 0.10 (DashMap-backed)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Input | mailrs-rate-limit | governor | Winner |
|---|---:|---:|---|
| hot key, allowed | **12.6 ns** | 13.8 ns | **mailrs +9%** ✅ |
| cold key first-touch | **155 ns** | 151 ns | **TIE** (−3 % noise) |

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

#### `mailrs-imap-proto` vs `imap-codec` 2.0-alpha (Rust nom-based IMAP codec)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Command | mailrs-imap-proto | imap-codec | Winner |
|---|---:|---:|---|
| `A001 SELECT INBOX` | **59 ns** | 61 ns | **mailrs +3%** (TIE) |
| `A002 FETCH 1:100 (FLAGS BODY[…])` | **104 ns** | 300 ns | **mailrs 2.87×** ✅ |
| `A003 LOGIN alice@example.com password` | **96 ns** | 110 ns | **mailrs +15%** ✅ |
| `A004 NOOP` | **32.5 ns** | 35.6 ns | **mailrs +10%** ✅ |

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

C library wiring is best-effort: libspf2 and opendkim aren't on brew,
so those rows need a source build. With the library installed,
`bench-harness/scripts/run-all.sh` picks it up.

### `mailrs-smtp-proto` (criterion, `cargo bench -p mailrs-smtp-proto`)

| Path | Median | Notes |
|---|---:|---|
| `parse_command/EHLO` | **6 ns** | was 22 ns; killed `verb.to_ascii_uppercase()` heap alloc |
| `parse_command/DATA` | **4 ns** | was 22 ns (and 16 ns vs smtp-codec 12 ns → loss); **−82%** |
| `parse_command/RCPT_TO` | **32 ns** | was 70 ns; same verb-buffer change |
| `parse_command/MAIL_FROM` | **66 ns** | was 103 ns; same |
| `parse_command/AUTH_PLAIN` | **11 ns** | |
| `format_ehlo_response` | **38 ns** | was 307 ns; commit `19aa482` replaced `write!`-macro dispatch with direct `push_str` of `&str` segments for **−89%** measured (~9× faster) |
| `address/is_valid_typical` | **6 ns** | |
| `address/split_typical` | **7 ns** | |
| `unstuff_data/1024b` | **168 ns** | was 371 ns; memchr scan **−55% / 2.2×** |
| `unstuff_data/10240b` | **2.82 µs** | was 5.00 µs; same change **−44% / 1.8×** (3.6 GB/s) |
| `unstuff_data/102400b` | **20.85 µs** | was 40.85 µs; same change **−49% / 2.0×** (4.9 GB/s) |

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
| `has_smuggle_sequence/clean_1024b` | **12.7 ns** | 81 GB/s | was 316 ns; memchr-anchored scan **−96 % / 25× faster** |
| `has_smuggle_sequence/clean_10240b` | **95 ns** | 108 GB/s | was 2.9 µs; **−97 % / 30×** |
| `has_smuggle_sequence/clean_102400b` | **907 ns** | 113 GB/s | was 28.5 µs; **−97 % / 31×** — close to memchr SIMD ceiling |
| `normalize_line_endings/lf_only` (12 B) | **55 ns** | — | unchanged — alloc-bound on tiny input |
| `normalize_line_endings/bare_lf_1024b` | **152 ns** | 6.7 GB/s | was 701 ns; memchr2 + chunked extend **−78 %** |
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
| `decode/line/login` | 22 B (`a001 LOGIN user pass\r\n`) | **72 ns** | — | — |
| `decode/line/select` | 19 B (`a002 SELECT INBOX\r\n`) | **67 ns** | — | |
| `decode/line/fetch_long` | 160 B (FETCH response with BODY metadata) | **107 ns** | 1.5 GB/s | line scaling reaches SIMD memchr floor |
| `decode/line/bare_cr_skip` | 24 B with 5 embedded bare `\r` | **76 ns** | — | exercises the memchr restart loop (RFC 9051 requires bare CR to be skipped) |
| `decode/literal/32b` | 32 B + CRLF | **62 ns** | — | minimal literal overhead |
| `decode/literal/1024b` | 1 KB + CRLF | **87.5 ns** | 12 GB/s | `BytesMut::split_to` + `to_vec` — single memcpy |
| `decode/literal/102400b` | 100 KB + CRLF | **13.2 µs** | **7.7 GB/s** | **memcpy ceiling** — split_to is zero-copy share, to_vec is the bound |
| `encode/short_12b` | 12 B | **38 ns** | — | one `extend_from_slice` to `BytesMut` |
| `encode/long_140b` | 140 B | **39.4 ns** | — | encode does not scale with payload — `BytesMut::extend_from_slice` is memcpy bound, dominated by setup overhead |

### `mailrs-imap-format` (criterion, `cargo bench -p mailrs-imap-format`)

| Path | Median | Notes |
|---|---:|---|
| `format_imap_flags/seen+answered` | **19 ns** | was 27.8 ns then 12.9 ns; current re-measure 19 ns sits between — noise variance. Structural win (no `Vec::push` + `join`) is unchanged. |
| `parse_imap_flags/seen answered` | **15 ns** | matches 16.1 ns figure within noise; `eq_ignore_ascii_case` against compile-time `&[u8; N]` targets, still load-bearing |
| `format_internal_date` | **177 ns** | dominated by `chrono` `from_timestamp` + format; squeeze deferred (would require an in-house date formatter) |
| `extract_header_section/body_1kb` | **78 ns** | was 130 ns; memchr-anchored separator scan **−40% / 1.66×** |
| `extract_header_section/body_5kb` | **79 ns** | was 129 ns; same change **−39% / 1.65×** (constant in body size — scanner stops at separator) |
| `extract_header_section/body_20kb` | **80 ns** | was 128 ns; same change **−37% / 1.59×** |
| `extract_body_section/body_1kb` | **95 ns** | was 132 ns; same change **−28% / 1.39×** (scan + Vec alloc for body output) |
| `extract_body_section/body_5kb` | **122 ns** | was 158 ns; **−23% / 1.30×** |
| `extract_body_section/body_20kb` | **1.37 µs** | was 1.41 µs; **−3% (noise)** — at 20 KB the output `Vec::to_vec` memcpy dominates, scan cost amortizes away |
| `find_line_offset/line_1` | **2.3 ns** | was 17.7 ns; **−87% / 7.7×** — short-skip case; memchr's SIMD startup overhead amortizes immediately on input → just 1 LF away |
| `find_line_offset/line_50` | **319 ns** | new bench coverage (no prior baseline) — typical FETCH `BODY[TEXT]<N.M>` partial-fetch shape |
| `find_line_offset/line_120` | **794 ns** | new bench coverage |

### `mailrs-smtp-client` (criterion, `cargo bench -p mailrs-smtp-client`)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Path | Median | Notes |
|---|---:|---|
| `sort_mx_records(20)` | **12 ns** | MX priority sort |
| `parse_response/short` | **24 ns** | was 27 ns; matches baseline within noise. unrolled 3-digit byte code parse still load-bearing |
| `parse_response/long_ehlo_10_lines` | **181 ns** | was 257 ns; numbers improved further (likely rustc / stdlib `lines()` improvement) |
| `dot_stuff(5 KB no dots)` | **~1.4 µs** | passthrough fast-path |
| `dot_stuff(5 KB with dots)` | **~1.6 µs** | allocates new Vec to escape |

### `mailrs-imap-proto` (criterion, `cargo bench -p mailrs-imap-proto`)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Path | Median | Notes |
|---|---:|---|
| `parse_command(LOGIN)` | **88 ns** | |
| `parse_command(SELECT)` | **56 ns** | |
| `parse_command(FETCH_uid_range)` | **100 ns** | `FETCH 1:1000 (FLAGS BODY.PEEK[HEADER])` |
| `parse_command(complex UID SEARCH)` | **164 ns** | |
| `sequence_set/parse_simple` | **134 ns** | `"1,3,5,7,9,11"` |
| `sequence_set/parse_ranges` | **110 ns** | `"1:100,200:300,400:500,*"` |
| `sequence_set_to_uids` (~2 K UIDs) | **5.8 µs** | real cost is `(1..=1000).collect() + (2000..=3000).collect() + sort + dedup` dominated by stable-sort + flat_map alloc. **Sort/dedup are necessary** for correctness; bench-name "n4001" is historical (real count is ~2002 elements). |

### `mailrs-jmap` (criterion, `cargo bench -p mailrs-jmap`)

| Path | Median | Notes |
|---|---:|---|
| `keywords_to_flags` | **~5.6 ns** | bitmask conversion |
| `dispatch_mailbox_get` | **3.58 µs** | |
| `dispatch_mailbox_query` | **478 ns** | |
| `dispatch_email_query` | **4.22 µs** | |
| `dispatch_thread_get` | **4.25 µs** | |
| `dispatch_request multi-call back-ref` | **15.1 µs** | |

### `mailrs-maildir` — Maildir delivery + flag parsing (criterion, M-series Mac, release)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Path | Median | Notes |
|---|---:|---|
| `parse_flags/empty` | **1.74 ns** | byte-iter scan over the cur/new entry suffix — already at noise floor |
| `parse_flags/seen_only` | **1.74 ns** | |
| `parse_flags/all_standard` | **1.75 ns** | |
| `deliver_loop/n=1` | **4.6 ms** | fs syscall bound (open + write + fsync + rename) — the floor is the filesystem, not the parser |
| `deliver_loop/n=8` | **39 ms** | per-message loop overhead linear in N |
| `deliver_batch/n=8` | **11.4 ms** | 8 messages batched in one fsync, **3.4× faster than loop** — the prior batch-fsync win still load-bearing |
| `deliver_batch/n=64` | **15.8 ms** | batch overhead amortizes — 20× per-message savings over deliver_loop at scale |

### `mailrs-mailbox` (criterion, `cargo bench -p mailrs-mailbox`)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Path | Median | Notes |
|---|---:|---|
| `add_flags` hot path | **52 ns** | DashMap entry update |
| `extract_message_id(short header)` | **59 ns** | was ~150 ns previously; memchr-anchored byte-level header walk replaced `from_utf8_lossy(data).lines()` |
| `extract_message_id(long header)` | **123 ns** | 20+ header lines; bounded by header count not body length |
| `mailbox_status` (1k messages) | **468 ns** | fixture-impl walks DashMap; PG impl pushes into SQL |
| `insert_message/first_insert` | **288 ns** | fixture insert + DashMap update |
| `insert_message/into_1k_mailbox` | **63 µs** | fixture cost — clones Message rows |
| `query_messages/by_mailbox_first_50` | **164 µs** | fixture cost — see PG comparison in README |
| `query_messages/text_match_1k` | **150 µs** | same — fixture-only; PG impl pushes search into SQL `WHERE` clause |

### `mailrs-rate-limit` (criterion, `cargo bench -p mailrs-rate-limit`)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Path | Median | Notes |
|---|---:|---|
| `evaluate_bucket/allowed` (pure math) | **1.55 ns** | GCRA TAT integer arithmetic, no I/O |
| `evaluate_bucket/denied_no_refill` | **1.57 ns** | |
| `check_hot_key/sync` | **12.9 ns** | bypass async trait |
| `check_hot_key/async` | **64 ns** | through `RateLimitStore` trait |
| `check_cold_key/first_touch` | **~150 ns** | DashMap insert path |
| `cleanup_stale(10k)` | **~145 µs** | batch scan + retain |

### `mailrs-shield` (criterion, `cargo bench -p mailrs-shield`)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Path | Median | Notes |
|---|---:|---|
| `dnsbl/reverse_ipv4` | **47 ns** | reverse IPv4 octet string build for DNSBL query |
| `dnsbl/interpret_spamhaus` | **524 ps** | bit-interpretation of Spamhaus A-record octets |
| `greylist/evaluate_first_seen` | **479 ps** | first-touch decision |
| `greylist/evaluate_retry` | **677 ps** | retry-window comparison |
| `greylist/triplet_key` | **25 ns** | was 120 ns previously; commit `d0c5941` replaced `format!` with pre-sized `String::with_capacity + push_str` (5× faster). Per inbound message on the greylist hot path |
| `ptr_score_from_names(match)` | **75 ns** | FCrDNS score eval |
| `ptr_score_from_names(no match)` | **205 ns** | DNS-mismatch slow path (extra HashSet ops) |

### `mailrs-spf` — RFC 7208 SPF verifier (criterion, M-series Mac, release)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Path | Median |
|---|---:|
| `Record::parse` (simple `v=spf1 ip4 -all`) | **46 ns** |
| `Record::parse` (complex 8-mechanism record) | **244 ns** |
| `verify` pass path (no real DNS) | **175 ns** |

Run: `cargo bench -p mailrs-spf --bench spf`. Production `verify` is
dominated by DNS round-trips (5-50 ms); the bench numbers above are
the pure CPU portion.

### `mailrs-dmarc` — RFC 7489 DMARC verifier + aggregate report (criterion, M-series Mac, release)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Path | Median | Notes |
|---|---:|---|
| `generate_xml/n10` | **7.74 µs** | was 13.2 µs; `write!()` rewrite **−41 % / 1.71×** |
| `generate_xml/n500` | **275 µs** | was 533 µs; same change **−48 % / 1.94×** (linear scaling) |
| `format_report_email` | **83 µs** | unchanged — already on `mailrs-mail-builder` since |
| `extract_rua_typical` | **70 ns** | tag-list scan via stdlib `split(';')` (stdlib uses memchr internally for `char` patterns) — already optimal |

### `mailrs-arc` — RFC 8617 ARC verifier (criterion, M-series Mac, release)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Path | Median | Notes |
|---|---:|---|
| `parse/aar` | **27 ns** | ARC-Authentication-Results parse |
| `parse/ams` | **541 ns** | ARC-Message-Signature parse |
| `parse/as` | **277 ns** | ARC-Seal parse |
| `chain/extract_two_hop` | **2.49 µs** | was 3.90 µs; memchr rewrite **−36 % / 1.57×** |

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

### `mailrs-mta-sts` — RFC 8461 STS policy parse + MX matcher (criterion, M-series Mac, release)

| Path | Median |
|---|---:|
| `parse/sts_record` | **~76 ns** |
| `parse/policy` | **~382 ns** |
| `mx_matches/literal` | **~44 ns** |
| `mx_matches/wildcard_match` | **~92 ns** |
| `mx_matches/wildcard_no_match` | **~89 ns** |
| `enforce/3_mx_first_match` | **~44 ns** |
| `enforce/3_mx_last_match` | **~219 ns** |

### `mailrs-tls-rpt` — RFC 8460 SMTP TLS reporting (criterion, M-series Mac, release)

| Path | Median |
|---|---:|
| `parse/record_single` | **~202 ns** |
| `parse/record_multi` | **~312 ns** |
| `report/build_100_success` | **~3.46 µs** |
| `report/build_mixed_100` | **~13.9 µs** |
| `report/serialize_json` | **~708 ns** |

### `mailrs-delivery-executor` — group-commit Maildir flusher (criterion, M-series Mac, release)

| Path | Median |
|---|---:|
| `DeliveryExecutor::spawn` | **~518 ns** |

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

### `mailrs-arf` — RFC 5965 ARF parser (criterion, M-series Mac, release)

| Path | Median |
|---|---:|
| `parse/hotmail_fbl_sample` | **~1.26 µs** |
| `is_arf/no_marker_short` | **~26 ns** |

Run: `cargo bench -p mailrs-arf --bench arf`.

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

Run: `cargo bench -p mailrs-rfc5322 --bench parse`.

### `mailrs-mail-builder` (criterion, `cargo bench -p mailrs-mail-builder`)

3-run medians (M-series Mac, release), measured 2026-06-03:

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

### `mailrs-sieve-core` (criterion, `cargo bench -p mailrs-sieve-core`)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Path | Before | After | Win |
|---|---:|---:|---:|
| `tokenize/typical` | 1.78 µs | 1.41 µs | **−21 %** |
| `tokenize/heavy` | 2.08 µs | 2.02 µs | noise |
| `compile/typical` | 2.89 µs | 2.86 µs | noise |
| `compile/heavy` | 4.25 µs | 4.04 µs | **−5 %** |
| `evaluate/typical` | 3.60 µs | 3.56 µs | noise |
| `evaluate/heavy` | 7.18 µs | 5.93 µs | **−17 %** |

### `mailrs-sieve` (criterion, `cargo bench -p mailrs-sieve`)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Path | Median | Notes |
|---|---:|---|
| `compile_sieve/typical` | **1.18 µs** | |
| `evaluate_sieve/typical` | **1.48 µs** | |

### `mailrs-attachment-extract` (criterion, `cargo bench -p mailrs-attachment-extract`)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Path | Median | Notes |
|---|---:|---|
| `extraction_method/text_plain` | **18.9 ns** | Content-Type byte-match dispatch |
| `extraction_method/application_pdf` | **24.3 ns** | same dispatch path |

### `mailrs-intelligence` (criterion, `cargo bench -p mailrs-intelligence`)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Path | Median | Notes |
|---|---:|---|
| `extract_structured_data/short_single_event` | **687 ns** | regex-free byte-scan for event / order patterns in body text |
| `extract_structured_data/long_with_flight_and_order` | **4.75 µs** | |
| `calculate_importance` | **2.93 ns** | integer-only score combination |

### `mailrs-postmaster` (criterion, `cargo bench -p mailrs-postmaster`)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Path | Median | Notes |
|---|---:|---|
| `extract_bimi_logo_url` | **40 ns** | BIMI TXT-record URL extraction |

### `mailrs-clean` (criterion, `cargo bench -p mailrs-clean`)

3-run medians (M-series Mac, release), measured 2026-06-03:

| Path | Median | Notes |
|---|---:|---|
| `clean_email_html/short_60b` | **10.5 µs** | constant-overhead floor (html2text setup) |
| `clean_email_html/marketing_500b` | **30 µs** | small marketing |
| `clean_email_html/marketing_5kb` | **144 µs** | |
| `clean_email_html/marketing_50kb` | **1.67 ms** | |
| `sender_heuristics/detect_bulk_sender_yes` | **27 ns** | regex-free byte scan |
| `sender_heuristics/is_automated_sender_yes` | **31 ns** | same path |
| `split_quoted_content` | **285 ns** | quote-line walk |

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
