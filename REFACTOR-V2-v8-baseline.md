# v8 baseline — pre-stone-wave-2 state freeze

> Captured 2026-05-27, before any v8 work begins. Every v8 ckpt's
> trigger compares against the numbers in this doc. Anything that
> regresses without explicit justification = ckpt blocked.
>
> Methodology: numbers measured on develop @ `55456ad`
> (post-v1.7.35 release + post-aws-lc-rs swap + post-ARCHITECTURE
> doc refresh). Comparing later requires the v8 work to be at the
> same git baseline minus its own additions.

## 1. Workspace functional state

### Source tree

| Item | Value |
|---|---|
| Workspace members | **42** (1 server bin + 41 published stones) |
| Latest tag | `v1.7.35` |
| Prod version | v1.7.35 (mail.golia.ai, `antispam=true`) |
| Prod uptime at capture | 3317 s |
| Last 24h inbound messages | 114 |

### Tests (`cargo test --workspace --no-fail-fast -- --skip _under_budget`)

| Metric | Value |
|---|---|
| Total tests passed | **3627** |
| Total `test result: ok` sections (binary × test-kind) | 152 |
| Failed | **0** |
| Workspace build (`cargo build --workspace`) | clean (last verified post-v1.7.35) |
| Clippy (`cargo clippy --workspace --all-targets -- -D warnings`) | clean (last verified post-v1.7.35) |

**v8 trigger contract**: every ckpt completion must keep this at
≥ 3627 passing + 0 failed. New tests welcome (count goes up); test
deletions require explicit justification in commit message.

### Lib coverage (`cargo llvm-cov --workspace --summary-only --lib`)

Workspace totals:

| Dimension | Coverage |
|---|---:|
| Lines | **83.40 %** (45 119 / 54 100) |
| Functions | **81.74 %** (3 683 / 4 506) |
| Regions | **80.97 %** (27 230 / 33 629) |

Per-file snapshot of v8-touched modules (so ckpt 0 / ckpt 3 / ckpt 6
can prove the swaps closed the gaps):

| Module | Lines | Functions |
|---|---:|---:|
| `outbound-queue/src/dsn.rs` | 100.00 % | 100.00 % |
| `outbound-queue/src/dkim_sign.rs` | 80.83 % | 82.05 % |
| `outbound-queue/src/queue.rs` | 30.80 % | 20.75 % |
| `outbound-queue/src/queue/suppression.rs` | 45.88 % | 33.33 % |
| `outbound-queue/src/store.rs` | 79.08 % | 59.26 % |
| `outbound-queue/src/pg_store.rs` | **0.00 %** | **0.00 %** |
| `outbound-queue/src/worker/mod.rs` | 64.04 % | 70.91 % |
| `outbound-queue/src/worker/delivery.rs` | **0.00 %** | **0.00 %** |
| `outbound-queue/src/worker/smtp.rs` | **0.00 %** | **0.00 %** |
| `smtp-client/src/connection.rs` | 38.73 % | 46.67 % |
| `smtp-client/src/dane.rs` | 65.80 % | 68.42 % |
| `smtp-client/src/mx.rs` | 90.30 % | 95.00 % |
| `smtp-client/src/response.rs` | 99.68 % | 100.00 % |
| `smtp-client/src/tls_outcome.rs` | 77.04 % | 88.89 % |
| `dmarc/src/lib.rs` | 97.33 % | 90.62 % |

**v8 trigger contract**:
- ckpt 0 → 1: outbound-queue/worker/* ≥ 80 % lines, pg_store.rs ≥ 80 %, smtp-client/connection.rs ≥ 70 %
- ckpt 3 (mail-builder swap): `outbound-queue/src/dsn.rs` and `dmarc/src/lib.rs` must stay ≥ baseline (100 % / 97.33 %).
- ckpt 6 (sieve swap): workspace overall coverage ≥ 83.40 % lines.
- Any per-crate lib coverage drop without explicit justification = ckpt blocked.

## 2. Performance state

### dkim_sign (current hot RSA-2048 sign path)

Measured 2026-05-27 with `cargo bench -p mailrs-outbound-queue --bench core -- --quick`
post-v1.7.35 aws-lc-rs swap. **The post-v8 numbers must stay within
±20 % of these absolute medians** (noise band):

| Bench | Median |
|---|---:|
| `dkim_sign/short` (200-byte message) | **288 µs** |
| `dkim_sign/long_8kb` (~8 KB body) | **309 µs** |
| `retry_delay_secs` (×10 calls) | 1.27 ns |
| `should_bounce` (×10 calls) | 1.27 ns |

**v8 trigger contract**: any ckpt that touches `mailrs-outbound-queue`,
`mailrs-dkim`, `mailrs-arc`, or `mailrs-mail-builder` re-runs this
bench and reports the new median + delta vs baseline. > +20 %
regression on a critical-path bench = ckpt blocked.

### Inbound + auth stones (PERFORMANCE.md recorded medians)

`mailrs-inbound`, `mailrs-spf`, `mailrs-dkim` (verify), `mailrs-arc`
(verify), `mailrs-dmarc` (evaluate) all already have per-file sections
in `PERFORMANCE.md` with median + budget. v8 work does not touch the
inbound pipeline — these are pure "must not regress" guard rails.

### Per-stone perf gates

41 stones × `tests/perf_gate.rs`. All currently pass under
`cargo test --release --workspace -- _under_budget`. dev-profile
under `cargo test --workspace` skips them (release.sh / CI both
`--skip _under_budget`).

**v8 trigger contract**: post-v8 `cargo test --release --workspace -- _under_budget`
must remain green.

### Prod metrics (current 11 names)

Reachable internally at `mailrs:3100/metrics`; histograms /
counters / gauges enumerated in v6 ckpt 4 walk
(`REFACTOR-V2-v6-ckpt4-security.md`). Most relevant for v8
regression-watch:

- `mailrs_outbound_delivery_seconds_bucket{outcome="delivered"}`
  — must not climb post-mail-builder swap (would mean we're
  generating malformed MIME that destination MTAs reject + retry)
- `mailrs_inbound_verdict_total{verdict="reject"}` — must not
  spike post-sieve swap (would mean differential test missed a
  case and sieve eval is rejecting more than before)
- `mailrs_outbound_queue_depth{status="pending"}` — must not grow
  monotonically post-any-swap (would mean delivery wedged)

## 3. Compliance state

### `cargo audit` (run 2026-05-27)

| Metric | Value |
|---|---|
| Dependencies scanned | 695 |
| Unhandled advisories | **0** |
| Documented ignores | 1 (`RUSTSEC-2023-0071` — `rsa` Marvin Attack, threat-model documented; **note: post-v1.7.35 the `rsa` crate is no longer in `mailrs-dkim` prod path — only in dev-deps for test fixtures**) |

### `cargo deny check`

advisories ok · bans ok · licenses ok · sources ok.

License exceptions:
1. `sieve-rs 0.7.x` AGPL-3.0 — documented compliance via repo publication (Apache-2.0 OR MIT)

**v8 trigger contract — ckpt 6 final**: deletes the AGPL exception
when `mailrs-sieve` swap lands. `cargo deny check licenses` must
remain green throughout transitional ckpts.

### OWASP top-10

10/10 ✅ from v6 ckpt 4 walk (`REFACTOR-V2-v6-ckpt4-security.md`).
v8 must maintain.

## 4. Public API surface (what downstream users see)

For each stone v8 touches, the published 1.x / 2.x / 3.x API must
stay backward-compatible OR be a SemVer-major bump documented with a
migration path. The stones in scope for v8:

| Stone | Current crates.io version | v8 plan |
|---|---|---|
| `mailrs-outbound-queue` | 2.0.0 (just bumped in v1.7.35 because dkim_sign API change) | ckpt 0 adds integration tests only — no API change |
| `mailrs-smtp-client` | 2.0.1 | ckpt 0 adds integration tests only — no API change |
| `mailrs-dmarc` | 2.x | ckpt 3 internal swap (`format_report_email` still wraps gzipped XML in multipart/mixed identically — output bytes equivalent) — no API change |
| `mailrs-mail-builder` (NEW) | — | ckpt 1 ships 0.1; ckpt 3 ships 1.0 to crates.io |
| `mailrs-sieve` (NEW) | — | ckpt 4 ships 0.1; ckpt 6 ships 1.0 to crates.io |

**v8 trigger contract**: every public API removal / type change in
existing stones requires a SemVer major bump in that crate's
Cargo.toml AND a CHANGELOG.md entry with the migration recipe.

## 5. CI state

| Workflow | Streak | Status |
|---|---|---|
| `test.yml` | last 5 success | green |
| `release.yml` | **4 consecutive success** (v1.7.32 → v1.7.35; cancelled v1.7.31 is the previous gap) | green |
| Average release.yml time | ~18 min | gate 8 min + docker 11 min + announce 1 min |

**v8 trigger contract**: release.yml must remain green throughout
v8 ship cadence. Any failed run blocks the corresponding ckpt's
sign-off.

## How to use this baseline at each ckpt

At ckpt completion, re-run:

```bash
# 1. functional (tests + coverage)
cargo test --workspace --no-fail-fast -- --skip _under_budget
cargo llvm-cov --workspace --summary-only --lib

# 2. perf (bench)
cargo bench -p <touched-stone> --bench <name> -- --quick

# 3. compliance
./scripts/check-security.sh

# 4. CI
gh run list --workflow=release.yml --limit 5
```

Compare each output against the row in the corresponding section of
this doc. If anything regressed:

1. Stop the ckpt; do not advance.
2. File the regression in the ckpt's commit message with the diff vs
   baseline.
3. Either fix the regression or pull back the ckpt (revert), then
   re-measure.

The point of the baseline: **"v8 closed" means every number above
either improved or stayed equal**. No "we accept this regression
because the new thing is more important" arguments unless explicitly
sanctioned by you, the user.
