# mailrs-clean

[![Crates.io](https://img.shields.io/crates/v/mailrs-clean?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-clean)
[![docs.rs](https://img.shields.io/docsrs/mailrs-clean?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-clean)
[![License](https://img.shields.io/crates/l/mailrs-clean?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-clean?style=flat-square)](https://crates.io/crates/mailrs-clean)

Email content cleanup primitives for Rust — multi-stage HTML sanitization, tracking-pixel detection, bulk/automated sender heuristics, and quoted-reply splitting. Zero I/O, no async runtime.

Extracted from [mailrs] so any mail client / inbound pipeline can run an email body through the same battle-tested pipeline that fronts a production server.

## What you get

| Entry point | What it does |
|---|---|
| [`clean_email_html`] | strip unsafe elements + tracking pixels + hidden blocks + marketing chrome, then convert to paragraph-aware plain text. Returns a [`CleanResult`] with the text plus flags for tracking / template-heavy / unsubscribe-link presence. |
| [`detect_bulk_sender`] | RFC 2369 `List-*` header heuristic — true for mailing-list / newsletter traffic. |
| [`is_automated_sender`] | local-part pattern check (`no-reply@`, `notification@`, …). |
| [`split_quoted_content`] | separate a fresh reply from quoted ancestry so UIs can collapse old context. |

## Quick start

```rust
use mailrs_clean::{clean_email_html, detect_bulk_sender, split_quoted_content};

let result = clean_email_html(
    r#"<html><body>
        <p>Hi! Big news today.</p>
        <img src="https://tracker.mailchimp.com/pixel.gif" width="1" height="1"/>
        <p style="display:none">spam block</p>
       </body></html>"#,
);
assert!(result.has_tracking_pixel);
// result.text is the cleaned, paragraph-aware plain text

let headers = "From: news@example.com\r\nList-Id: <news.example.com>\r\n";
assert!(detect_bulk_sender(headers));

let (fresh, quoted) = split_quoted_content(
    "Sounds good.\n\nOn Wed wrote:\n> Could we move it to 10am?",
);
// fresh = "Sounds good.", quoted = vec!["Could we move it to 10am?"]
```

## What the HTML cleaner removes

- `<script>`, `<style>`, `<iframe>`, `<object>`, `<embed>` — unsafe.
- `<img width=1 height=1>` from known tracking domains (Mailchimp, SendGrid, HubSpot, Mailgun, Amazon SES, …) — privacy.
- Blocks with `display:none`, `visibility:hidden`, `opacity:0` — usually hidden spam-keyword harvesting.
- Marketing-template chrome: outer `<table>` wrappers around a single content block, repeated unsubscribe footers.
- After cleanup, the residue goes through `html2text` for paragraph-aware plain-text conversion.

Tracking-domain list is `const` and exhaustive for the top 18 commercial email-marketing platforms — see `lib.rs` for the current list.

## Performance

[`benches/clean.rs`](benches/clean.rs) covers the HTML cleaner across realistic email sizes plus the sender heuristics + quote splitter. Measured with criterion 0.8 on Apple Silicon (M-series), `cargo bench`, release profile.

| Operation | Input size | Median | Throughput |
|---|---|---|---|
| `clean_email_html` (short body) | 60 B | ~20 µs | — |
| `clean_email_html` (short marketing) | 560 B | ~56 µs | ~10 MB/s |
| `clean_email_html` (5 KB marketing) | 5.6 KB | ~336 µs | ~17 MB/s |
| `clean_email_html` (50 KB worst-case) | 56 KB | ~2.5 ms | ~22 MB/s |
| `detect_bulk_sender` (with List-Id) | — | ~60 ns | — |
| `is_automated_sender("no-reply@…")` | — | ~50 ns | — |
| `is_automated_sender(human address)` | — | ~50 ns | — |
| `split_quoted_content` (typical reply) | — | ~970 ns | — |

The cost is dominated by `scraper` parser construction + tree walk; the per-byte rate is roughly flat across input sizes once parser startup is amortized. For workloads that process every inbound message at SMTP time, this means a 50 KB worst-case marketing email costs ~2.5 ms of CPU on top of the DATA-stage I/O.

Run with `cargo bench -p mailrs-clean`. See [`tests/perf_gate.rs`](tests/perf_gate.rs) for the regression budgets.

## What this is NOT

- Not a general HTML sanitizer — for that, use [`ammonia`]. We make email-specific assumptions (single-document message, no script execution, structured-output text).
- Not a parser for MIME bodies — bring your own (`mail-parser`, `lettre`, etc.); feed us the `text/html` part.
- Not opinionated about how you score senders — the heuristics return booleans, the policy decisions are yours.

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-clean`) |
| **test** | line cov: 91.6% (`cargo llvm-cov -p mailrs-clean --summary-only`) |
| **bench** | ✅ 1 file(s) criterion + ✅ 3 gate(s) `perf_gate.rs` |
| **size** | release rlib: 1.3 MB |
| **fuzz** | ❌ none |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons

- Searched crates.io + competing impls: see PERFORMANCE.md or 'first-in-Rust' marker.

<!-- AUDIT-FOOTER:END -->

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

[mailrs]: https://github.com/goliajp/mailrs
[`ammonia`]: https://crates.io/crates/ammonia
