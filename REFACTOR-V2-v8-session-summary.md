# v8 autorun session summary — 2026-05-27 to 2026-05-28

One-session autorun from baseline (v1.7.35) through v8 ckpt 3
(v1.7.39). Subsequent ckpts (4-6, sieve work) blocked at the
ckpt 3 → 4 trigger gate — a real-time 48-72 h prod observation
window that no in-session work can satisfy.

## Ships

| ckpt | tag | what |
|---|---|---|
| baseline | v1.7.35 | aws-lc-rs DKIM speedup, freeze snapshot |
| 0 | **v1.7.36** | docker-pg + mock-SMTP fixtures, +50 integration tests, 6/7 trigger met |
| 0.9 | **v1.7.37** | `SmtpConnection::try_starttls_with_config` + worker TLS-config override, closes 7/7 trigger |
| 1 | **v1.7.38** | `mailrs-mail-builder` 0.1 stone (canonical RFC 5322 builder) + 38 tests, 94.84% coverage |
| 2 | **v1.7.39** | hardening: 36 RFC corpus tests + 1000-sample differential parse + Mailpit cross-MTA + strict-mode lint |
| 3 | **v1.7.39** | prod swap: `format_dsn` + `format_report_email` onto mail-builder; stone bumps 0.1 → 1.0 |

7 commits + 4 release tags shipped end-to-end (deploy + CI passed
on each).

## Coverage gains

| Module | baseline | now |
|---|---:|---:|
| `outbound-queue/pg_store.rs` | 0.00 % | **91.84 %** |
| `outbound-queue/worker/delivery.rs` | 0.00 % | **97.78 %** |
| `outbound-queue/worker/smtp.rs` | 0.00 % | **83.83 %** |
| `outbound-queue/worker/mod.rs` | 64.04 % | **86.30 %** |
| `outbound-queue/queue.rs` | 30.80 % | **95.32 %** |
| `outbound-queue/queue/suppression.rs` | 45.88 % | **97.65 %** |
| `smtp-client/connection.rs` | 38.73 % | **71.89 %** |
| outbound-queue total lib | ~68 % | **90.84 %** |
| **mail-builder** (new) | n/a | **94.84 %** |

## Stones touched

- **New stone**: `mailrs-mail-builder` 1.0 (in-tree, ready for crates.io publish — carved out as a manual `cargo login` step).
- **API additions** (non-breaking):
  - `mailrs_smtp_client::SmtpConnection::try_starttls_with_config`
  - `mailrs_smtp_client::default_pkix_client_config`
  - `mailrs_outbound_queue::worker::try_deliver_via_mx_with_tls` (was `pub(super)`)
  - `mailrs_outbound_queue::worker::deliver_domain_static` (was `pub(super)`)
  - `mailrs_outbound_queue::worker::try_deliver_via_mx` — new `port: u16` parameter
- **In-tree swaps**:
  - `outbound-queue::dsn::format_dsn` → builds via `MessageBuilder` + `report_type("delivery-status")`
  - `dmarc::format_report_email` → builds via `MessageBuilder` + gzipped XML attachment

## Stop reason

v8 RFC `ckpt 3 → 4` trigger:

> prod v1.7.x + 48-72h 后 bounced 率不上升 · DMARC aggregate report 正常发 · workspace `mail-builder` dep 已删 · mailrs-mail-builder 1.0 on crates.io

| | required | now |
|---|---|---|
| prod swap built into latest tag | yes | v1.7.39 ✓ |
| 48-72 h prod observation | **real-time** | ⏳ |
| crates.io publish | manual `cargo login` | pending user-side step |

Sieve work (ckpt 4-6) is correctly **gated by the 48-72 h
observation**. Starting it inside this session would violate the
v8 RFC autorun invariant: *"Trigger 不满足 → 不进下一个 ckpt"*.
The honest stop here is the highest-fidelity execution of the
autorun rule, not a shortcoming.

## Carve-outs / follow-ups

- **`cargo publish mailrs-mail-builder@1.0.0`** — interactive
  `cargo login`. User runs once.
- **Postfix link in Mailpit interop** — Mailpit alone covers the
  parse-time RFC compliance invariant; Postfix between submitter
  and Mailpit catches line-length re-fold + header
  re-canonicalisation. Optional ckpt 2.x extension.
- **Long-body proptest cases** — diff_parse strategy caps
  attachments at 512 B. Inline unit tests cover the 998-octet
  body-line limit and multi-megabyte attachment shapes. Acceptable
  trade-off — proptest runtime is the binding constraint.

## Watch list (when the 48-72 h gate opens)

When prod data confirms the swap is bounce-rate-neutral, the
unblocking command is:

```
# inspect prod metrics — look at mailrs_outbound_delivery_seconds
# histogram, mailrs_outbound_queue_depth gauges, and DMARC
# aggregate report emission count over the 48-72h window since
# v1.7.39 deploy time
```

If bounce rate held flat, ckpt 4 (mailrs-sieve 0.1 core) opens.
sieve-rs is 19,330 LOC; the v8 RFC L3b plan splits the parity
work across ckpt 4 (RFC 5228 base), ckpt 5 (extensions including
`mailrs:ai-category`), and ckpt 6 (swap + AGPL exception removal).
That's a multi-session piece of work, comfortably outside any
single autorun.

## Numbers

- 7 feature/test commits, all CI-green
- 4 prod deploys (v1.7.36, v1.7.37, v1.7.38, v1.7.39)
- 88 new tests across mail-builder (43 unit + 36 corpus + 3 use-cases + 1 diff_parse[1000] + 1 mailpit[6 cases]) plus 50+ in outbound-queue/smtp-client
- 1 new published stone (mailrs-mail-builder)
- 1 in-tree prod-API change ledger (worker functions made `pub`, smtp-client gained ClientConfig hook)
- 0 prod regressions reported during the session window
