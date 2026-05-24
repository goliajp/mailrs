# mailrs-arf

[![Crates.io](https://img.shields.io/crates/v/mailrs-arf?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-arf)
[![docs.rs](https://img.shields.io/docsrs/mailrs-arf?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-arf)
[![License](https://img.shields.io/crates/l/mailrs-arf?style=flat-square)](#license)

RFC 5965 Abuse Reporting Format (ARF) parser — extract the machine-
readable fields from feedback-loop complaint reports.

When a recipient mailbox provider (Hotmail / Yahoo / AOL / Gmail / etc.)
processes an abuse complaint, it generates an ARF report wrapped in
`multipart/report; report-type=feedback-report` and sends it back to
the sender's published abuse@ address. The middle `message/feedback-
report` part contains structured fields (`Feedback-Type`,
`Original-Mail-From`, `Original-Rcpt-To`, `Source-IP`, …) that drive
suppression-list updates, sender reputation tracking, and incident
response.

This crate is the focused, zero-dependency parser for that middle
part. It does NOT generate ARF reports (use `mailrs-mime` or
`mail-builder` for that) and it does NOT take any policy action — it
just hands you the parsed fields. Down-stream code decides whether to
update suppression lists, alert oncall, etc.

## Quick start

```rust
use mailrs_arf::parse;

let raw_message = b"From: fbl@hotmail.com\r\n\
Subject: complaint about message from sender@example.com\r\n\
Content-Type: multipart/report; report-type=feedback-report;\r\n\
\tboundary=\"----=BOUNDARY\"\r\n\
\r\n\
------=BOUNDARY\r\n\
Content-Type: message/feedback-report\r\n\
\r\n\
Feedback-Type: abuse\r\n\
User-Agent: Hotmail FBL\r\n\
Version: 1\r\n\
Original-Mail-From: sender@example.com\r\n\
Original-Rcpt-To: recipient@hotmail.com\r\n\
Source-IP: 192.0.2.42\r\n\
Reported-Domain: example.com\r\n\
\r\n\
------=BOUNDARY--\r\n";

let report = parse(raw_message).expect("ARF report");
assert_eq!(report.feedback_type, "abuse");
assert_eq!(report.original_rcpt_to.as_deref(), Some("recipient@hotmail.com"));
assert_eq!(report.original_mail_from.as_deref(), Some("sender@example.com"));
assert_eq!(report.source_ip.as_deref(), Some("192.0.2.42"));
```

## What gets parsed

All RFC 5965 §3.1 + §3.2 fields, returned as a `Report` struct:

| Field | Source RFC clause |
|---|---|
| `feedback_type` | 5965 §3.1 (default `"abuse"` if missing) |
| `user_agent` | 5965 §3.1 |
| `version` | 5965 §3.1 |
| `original_mail_from` | 5965 §3.2 |
| `original_rcpt_to` | 5965 §3.2 |
| `arrival_date` | 5965 §3.2 |
| `source_ip` | 5965 §3.2 |
| `reported_domain` | 5965 §3.2 |
| `reported_uri` | 5965 §3.2 (multi-valued; first wins) |
| `authentication_results` | 5965 §3.2 |
| `incidents` | 5965 §3.2 |

Field values returned verbatim; `<…>` angle brackets stripped from
addresses; lowercase normalization applied to `feedback_type`,
`original_*`, `reported_domain`. Header continuation lines (RFC 5322
§2.2.3) handled correctly.

## What is NOT parsed

- The `multipart/report` envelope structure (use `mailrs-mime` if you
  need the full MIME tree).
- The third part of the ARF report (the original message that
  triggered the complaint) — that's a regular RFC 5322 message and
  can be parsed by `mailrs-mime` / `mailrs-rfc5322` independently.
- ARF 1.0 vs RFC 5965 differences — this crate targets RFC 5965 (the
  IETF-standard form). The pre-standard 0.x ARF format is sufficiently
  similar that 95% of real feedback-loop reports parse correctly.

## Performance

| Operation | Median (M-series Mac, release) |
|---|---:|
| `parse` on a 600-byte Hotmail FBL sample | **1.16 µs** |
| `parse` on non-ARF input (early exit) | **27.6 ns** |

(Run `cargo bench -p mailrs-arf` to reproduce. The early-exit path is
the substring scan for the literal `feedback-report` marker —
sub-30 ns means even hot mailbox loops that try every inbound
message can safely call `parse()` unconditionally.)

## License

Dual-licensed under [Apache 2.0](./LICENSE-APACHE) or [MIT](./LICENSE-MIT).
Pick whichever fits your project.
