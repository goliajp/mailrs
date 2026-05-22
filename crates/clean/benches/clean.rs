//! Microbenchmarks for the html-clean pipeline.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_clean::{clean_email_html, detect_bulk_sender, is_automated_sender, split_quoted_content};

const SHORT_HTML: &str = "<html><body><p>Hello,</p><p>This is a quick note.</p></body></html>";

const MARKETING_HTML: &str = r#"<html><head><style>body{font-family:sans-serif}</style></head><body>
<table width="600"><tr><td style="background:#fff;padding:20px">
  <h1>Big Sale</h1>
  <p>Save up to 50%! <a href="https://shop.example.com">Shop now</a></p>
  <img src="https://tracker.mailchimp.com/track/pixel.gif?id=abc" width="1" height="1" />
  <div style="display:none">Spam keyword harvesting block</div>
  <p style="visibility:hidden">Hidden CTA</p>
  <p><a href="https://example.com/unsubscribe">Unsubscribe</a></p>
</td></tr></table>
</body></html>"#;

const QUOTED_REPLY: &str = "Sounds good, see you then.

On Wed, May 20, 2026 at 9:00 AM, Bob <bob@example.com> wrote:
> Could we shift the meeting to 10am?
> Thanks,
> Bob
>
> > Sure, originally planned 9am.
> > -- alice";

/// ~5 KB synthetic Mailchimp-shape marketing email — multiple sections,
/// inline styles, hidden divs, tracking pixel, unsubscribe link.
fn marketing_email_5kb() -> String {
    let section = r#"<table cellpadding="0" cellspacing="0" border="0" width="600" align="center" style="background:#ffffff;margin:0 auto;">
<tr><td style="padding:24px 32px;font-family:Helvetica,Arial,sans-serif;color:#222;">
  <h2 style="font-size:22px;margin:0 0 12px 0;">Section title goes here</h2>
  <p style="font-size:15px;line-height:1.5;">Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris.</p>
  <p style="font-size:15px;line-height:1.5;">Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. <a href="https://shop.example.com/x" style="color:#0066cc;">Read more &rarr;</a></p>
  <img src="https://cdn.example.com/promo.png" width="536" height="280" alt="promo" style="display:block;width:100%;height:auto;" />
</td></tr></table>"#;
    let mut html = String::with_capacity(6 * 1024);
    html.push_str(r#"<!doctype html><html><head><meta charset="utf-8" /><title>Newsletter</title><style>body{margin:0;padding:0;background:#f4f4f4;font-family:Helvetica,Arial,sans-serif}</style></head><body style="margin:0;padding:0;background:#f4f4f4;">"#);
    html.push_str(r#"<div style="display:none;font-size:1px;color:#fefefe;line-height:1px;max-height:0;max-width:0;opacity:0;overflow:hidden;">Preview text that won't be visible in the email body itself.</div>"#);
    for _ in 0..5 {
        html.push_str(section);
    }
    html.push_str(r#"<img src="https://tracker.mailchimp.com/track/open.gif?u=abc&id=123" width="1" height="1" border="0" style="display:none" />"#);
    html.push_str(r#"<p style="font-size:11px;color:#888;text-align:center;padding:12px;">You received this because you subscribed. <a href="https://example.com/unsubscribe?t=xyz">Unsubscribe</a></p>"#);
    html.push_str("</body></html>");
    html
}

/// ~50 KB worst-case payload — long marketing email with many sections,
/// embedded tracker pixels, and inline style blobs. Stresses the parser
/// + style-attribute stripping.
fn large_marketing_html_50kb() -> String {
    let section = marketing_email_5kb();
    let mut html = String::with_capacity(60 * 1024);
    for _ in 0..10 {
        html.push_str(&section);
    }
    html
}

fn bench_clean_html(c: &mut Criterion) {
    let marketing_5kb = marketing_email_5kb();
    let large_50kb = large_marketing_html_50kb();

    let mut group = c.benchmark_group("clean_email_html");
    group.bench_function("short_60b", |b| {
        b.iter(|| clean_email_html(black_box(SHORT_HTML)))
    });
    group.bench_function("marketing_500b", |b| {
        b.iter(|| clean_email_html(black_box(MARKETING_HTML)))
    });
    group.bench_function("marketing_5kb", |b| {
        b.iter(|| clean_email_html(black_box(&marketing_5kb)))
    });
    group.bench_function("marketing_50kb", |b| {
        b.iter(|| clean_email_html(black_box(&large_50kb)))
    });
    group.finish();
}

fn bench_sender_heuristics(c: &mut Criterion) {
    let mut group = c.benchmark_group("sender_heuristics");
    let headers = "From: news@example.com\r\nList-Id: <news.example.com>\r\nList-Unsubscribe: <mailto:unsub@x>\r\n";
    group.bench_function("detect_bulk_sender_yes", |b| {
        b.iter(|| detect_bulk_sender(black_box(headers)))
    });
    group.bench_function("is_automated_sender_yes", |b| {
        b.iter(|| is_automated_sender(black_box("no-reply@example.com")))
    });
    group.bench_function("is_automated_sender_no", |b| {
        b.iter(|| is_automated_sender(black_box("alice@example.com")))
    });
    group.finish();
}

fn bench_split_quoted(c: &mut Criterion) {
    c.bench_function("split_quoted_content", |b| {
        b.iter(|| split_quoted_content(black_box(QUOTED_REPLY)))
    });
}

criterion_group!(benches, bench_clean_html, bench_sender_heuristics, bench_split_quoted);
criterion_main!(benches);
