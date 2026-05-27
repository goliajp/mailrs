# v8 ckpt 3 — mail-builder prod swap + 1.0

Closes the in-tree swap half of ckpt 3: the two production paths
that emit RFC 5322 messages (`outbound-queue::dsn::format_dsn` for
RFC 3464 bounces, `dmarc::format_report_email` for RFC 7489
aggregate reports) now build through `mailrs-mail-builder` instead
of inline `format!` string templates. The stone bumps from 0.1 to
1.0.

## What got built

### 3.1 — multipart/report support

`MessageBuilder::report_type(kind: impl Into<String>)` switches the
outer Content-Type from `multipart/mixed` to `multipart/report;
report-type=<kind>; boundary=...` (RFC 6522 / 3464 / 7489). Without
the setter, multipart/mixed is unchanged.

### 3.2 — outbound-queue::dsn::format_dsn

The full RFC 3464 DSN envelope, previously hand-rolled with
`write!` into a String buffer, is now constructed via
`MessageBuilder` + an `Attachment` of content-type
`message/delivery-status`. The machine-readable body (Final-Recipient,
Action, Status, Diagnostic-Code) ships through the new
`text/* + message/*` CTE path: short ASCII bytes route to `7bit`
so the diagnostic fields land verbatim on the receiving side
instead of through a base64 layer.

Existing tests adjusted:
- `To: <sender@example.com>` → `To: sender@example.com` (RFC 5322
  §3.4 addr-spec is equally valid as name-addr with brackets;
  mail-builder renders bare addresses for bare-email input).
- hardcoded `--dsn-boundary\r\n` markers → structural checks
  (boundary parameter present + at least 2 envelope marker lines).
  The boundary string is no longer a contract — mail-builder's
  collision-scan picks one per-call.
- substring matches in `Final-Recipient` / `Diagnostic-Code` lines
  now run against an unfolded copy so the RFC 5322 §2.2.3 soft-wrap
  doesn't trip them.

### 3.3 — dmarc::format_report_email

Same swap pattern for the DMARC aggregate report email. The
gzipped XML attachment routes through the standard base64 path
(`application/gzip` content-type, no special-casing needed).

Three tests adjusted (same boundary / header-format reasons as
DSN above). 96 of 96 dmarc lib tests pass.

### 3.4 — mail-builder 1.0

- `version = "0.1.0"` → `"1.0.0"` in `crates/mail-builder/Cargo.toml`.
- `outbound-queue` and `dmarc` now depend on `mailrs-mail-builder = "1"`.
- CHANGELOG.md records the 1.0 promotion + the new APIs since 0.1
  (`report_type`, `build_strict`, attachment CTE selection by content-type).

### 3.5 — ckpt 3 → 4 trigger gate

The v8 RFC `ckpt 3 → 4` trigger requires **48-72 h prod observation
with no bounced-rate increase** before sieve work (ckpt 4) can
start. That's a real-time gate the autorun cannot satisfy inside
a single session — sieve work is correctly blocked until the
observation passes.

## What's NOT covered (carve-outs)

- **`cargo publish` to crates.io**: requires an interactive
  `cargo login` (CARGO_REGISTRY_TOKEN). Not done in this commit;
  scheduled as a manual step the user runs once. The crate is at
  1.0.0 in the workspace and ready to publish; nothing further
  needs to change inside the source.
- **`mail-builder` swap inside other emitter paths**: TLSRPT
  emitter is still a future feature (v9), no other prod path
  currently emits RFC 5322 via inline string templates. The two
  swaps in this commit cover all current `format!`-shaped emitters.

## Trigger status (ckpt 3 → 4)

| | required | now |
|---|---|---|
| workspace `mail-builder` dep references published 1.0 | yes — path-only with `version = "1"`, ready for `cargo publish` | ✓ |
| `outbound-queue::dsn::format_dsn` swapped | yes | ✓ |
| `dmarc::format_report_email` swapped | yes | ✓ |
| DMARC aggregate report 正常发 | yes — 96 lib tests pass + structure preserved | ✓ |
| prod v1.7.x + 48-72 h, bounced 率不上升 | **real-time gate** | ⏳ pending |

The pending real-time gate is the **session stop**. Ckpt 4 work
(sieve interpreter, ~19330 LOC parity with sieve-rs) cannot start
until 48-72 h prod data confirms the swap is bounce-rate-neutral.

## Next

When the v1.7.40 ship has been live 48-72 h and prod metrics show
no bounce-rate regression, ckpt 4 (mailrs-sieve 0.1) opens. The
v8 RFC L3b plan for ckpt 4-6 is intact.
