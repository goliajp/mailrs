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

fn bench_clean_html(c: &mut Criterion) {
    let mut group = c.benchmark_group("clean_email_html");
    group.bench_function("short", |b| b.iter(|| clean_email_html(black_box(SHORT_HTML))));
    group.bench_function("marketing", |b| {
        b.iter(|| clean_email_html(black_box(MARKETING_HTML)))
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
