# mailrs-clean

[![Crates.io](https://img.shields.io/crates/v/mailrs-clean?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-clean)
[![docs.rs](https://img.shields.io/docsrs/mailrs-clean?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-clean)
[![License](https://img.shields.io/crates/l/mailrs-clean?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-clean?style=flat-square)](https://crates.io/crates/mailrs-clean)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

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

## What this is NOT

- Not a general HTML sanitizer — for that, use [`ammonia`]. We make email-specific assumptions (single-document message, no script execution, structured-output text).
- Not a parser for MIME bodies — bring your own (`mail-parser`, `lettre`, etc.); feed us the `text/html` part.
- Not opinionated about how you score senders — the heuristics return booleans, the policy decisions are yours.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

[mailrs]: https://github.com/goliajp/mailrs
[`ammonia`]: https://crates.io/crates/ammonia
