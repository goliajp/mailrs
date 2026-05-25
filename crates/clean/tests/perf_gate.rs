//! Performance regression gates. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_clean::{clean_email_html, detect_bulk_sender, split_quoted_content};

const ITERS: usize = 100;

fn time_median<F: FnMut()>(mut op: F) -> Duration {
    let mut samples = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let start = Instant::now();
        op();
        samples.push(start.elapsed());
    }
    samples.sort();
    samples[ITERS / 2]
}

const MARKETING_HTML: &str = r#"<html><head><style>body{font-family:sans-serif}</style></head><body>
<table width="600"><tr><td style="background:#fff;padding:20px">
  <h1>Big Sale</h1>
  <p>Save up to 50%! <a href="https://shop.example.com">Shop now</a></p>
  <img src="https://tracker.mailchimp.com/track/pixel.gif?id=abc" width="1" height="1" />
  <div style="display:none">Spam keyword harvesting block</div>
  <p><a href="https://example.com/unsubscribe">Unsubscribe</a></p>
</td></tr></table></body></html>"#;

const QUOTED_REPLY: &str = "Sounds good, see you then.

On Wed, May 20, 2026 at 9:00 AM, Bob <bob@example.com> wrote:
> Could we shift the meeting to 10am?
> Thanks, Bob
>
> > Sure, originally planned 9am.
> > -- alice";

#[test]
fn clean_email_html_marketing_under_budget() {
    let median = time_median(|| {
        let _ = clean_email_html(MARKETING_HTML);
    });
    // Budget: 5 ms. Observed P95: ~250 µs.
    let budget = Duration::from_millis(5);
    assert!(
        median < budget,
        "clean_email_html median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn detect_bulk_sender_under_budget() {
    let headers = "From: news@example.com\r\nList-Id: <news.example.com>\r\nList-Unsubscribe: <mailto:unsub@x>\r\n";
    let median = time_median(|| {
        let _ = detect_bulk_sender(headers);
    });
    // Budget: 50 µs. Observed P95: ~2 µs.
    let budget = Duration::from_micros(50);
    assert!(
        median < budget,
        "detect_bulk_sender median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn split_quoted_content_under_budget() {
    let median = time_median(|| {
        let _ = split_quoted_content(QUOTED_REPLY);
    });
    // Budget: 500 µs. Observed P95: ~20 µs.
    let budget = Duration::from_micros(500);
    assert!(
        median < budget,
        "split_quoted_content median {median:?} exceeded {budget:?}"
    );
}
