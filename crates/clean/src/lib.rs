#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Email content cleanup primitives — HTML → readable text + sender heuristics.
//!
//! Four entry points cover what a mail client / inbound pipeline
//! typically needs after parsing an RFC 5322 message:
//!
//! - [`clean_email_html`] — multi-stage HTML pipeline that strips
//!   tracking pixels, hidden blocks, marketing-template chrome, and
//!   unsafe elements, then converts what's left to a paragraph-aware
//!   plain-text view. Returns [`CleanResult`] with the cleaned text
//!   plus boolean flags the caller can fold into an importance /
//!   spam score.
//! - [`detect_bulk_sender`] — RFC 2369 `List-*` header heuristic, used
//!   to demote mailing-list traffic in inbox sorting.
//! - [`is_automated_sender`] — local-part pattern check for
//!   `no-reply@`, `notification@`, etc.
//! - [`split_quoted_content`] — separate a fresh reply from its quoted
//!   ancestry so UIs can collapse old context.
//!
//! Zero I/O, no async runtime — give it strings, get strings back.

/// known tracking pixel domains
pub(crate) const TRACKING_DOMAINS: &[&str] = &[
    "mailchimp.com", "sendgrid.net", "hubspot.com", "mailgun.org",
    "constantcontact.com", "campaign-archive.com", "list-manage.com",
    "exacttarget.com", "sailthru.com", "marketo.com", "pardot.com",
    "braze.com", "iterable.com", "customer.io", "intercom-mail.com",
    "mandrillapp.com", "amazonses.com", "postmarkapp.com",
];

/// tracking pixel url path keywords
pub(crate) const TRACKING_PATHS: &[&str] = &[
    "/track", "/pixel", "/beacon", "/open", "/wf/open", "/o/", "/t/",
    "/imp", "/ci/", "/e/o/", "tracking", "1x1",
];

/// footer keywords (multi-language)
pub(crate) const FOOTER_KEYWORDS: &[&str] = &[
    "unsubscribe", "opt-out", "opt out", "manage preferences",
    "email preferences", "update preferences", "subscription",
    "配信停止", "退订", "取消订阅", "メール配信", "購読解除",
    "view in browser", "view this email", "ブラウザで表示",
    "privacy policy", "terms of service", "all rights reserved",
    "©", "you are receiving this",

    "this email was sent to", "no longer wish to receive",
    "if you no longer", "to stop receiving",
];

/// Result of [`clean_email_html`].
///
/// The cleaned text plus signals the caller can fold into a spam / importance score.
pub struct CleanResult {
    /// Plain text rendering of the cleaned HTML (tracking removed, footer
    /// chrome stripped, paragraphs preserved).
    pub clean_text: String,
    /// `true` when at least one tracking pixel was detected and stripped.
    pub has_tracking_pixel: bool,
    /// `true` when the HTML looked like a marketing template (heavy on
    /// inline styles, hidden divs, table-based layout).
    pub is_template_heavy: bool,
    /// Number of `<a>` tags in the original HTML.
    pub link_count: usize,
    /// Number of `<img>` tags in the original HTML.
    #[allow(dead_code)]
    pub image_count: usize,
    /// Ratio of plain-text bytes to total HTML bytes — useful for spotting
    /// emails that are mostly chrome.
    #[allow(dead_code)]
    pub text_to_html_ratio: f32,
}


/// clean html email content through multi-stage pipeline
pub fn clean_email_html(html: &str) -> CleanResult {
    let has_tracking_pixel = detect_tracking_pixels(html);
    let link_count = count_pattern(html, "<a ");
    let image_count = count_pattern(html, "<img ");

    // stage 1: remove script, style, comments
    let stage1 = remove_unsafe_elements(html);

    // stage 2: remove tracking pixels and hidden elements
    let stage2 = remove_tracking_and_hidden(&stage1);

    // stage 3: remove template header/footer
    let (stage3, footer_removed) = remove_template_chrome(&stage2);

    // stage 4: convert to clean text
    let clean_text = html_to_clean_text(&stage3);

    let text_to_html_ratio = if html.is_empty() {
        1.0
    } else {
        clean_text.len() as f32 / html.len() as f32
    };

    // template-heavy: low text ratio + many images or footer removed
    let is_template_heavy = text_to_html_ratio < 0.05 || (footer_removed && text_to_html_ratio < 0.15);

    CleanResult {
        clean_text,
        has_tracking_pixel,
        is_template_heavy,
        link_count,
        image_count,
        text_to_html_ratio,
    }
}

/// detect tracking pixels in raw html

mod html;
mod quote;
mod sender;

use html::{count_pattern, 
    detect_tracking_pixels, html_to_clean_text,
    remove_template_chrome, remove_tracking_and_hidden, remove_unsafe_elements,
};
pub use quote::split_quoted_content;
pub use sender::{detect_bulk_sender, is_automated_sender};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_simple_html() {
        let html = "<html><body><p>Hello world</p></body></html>";
        let result = clean_email_html(html);
        assert!(result.clean_text.contains("Hello world"));
        assert!(!result.has_tracking_pixel);
    }

    #[test]
    fn detect_1x1_tracking_pixel() {
        let html = r#"<img src="https://track.mailchimp.com/open?u=123" width="1" height="1">"#;
        assert!(detect_tracking_pixels(html));
    }

    #[test]
    fn detect_hidden_pixel() {
        let html = r#"<img src="https://example.com/pixel" style="display:none">"#;
        assert!(detect_tracking_pixels(html));
    }

    #[test]
    fn no_tracking_pixel_normal_image() {
        let html = r#"<img src="https://example.com/photo.jpg" width="600" height="400">"#;
        assert!(!detect_tracking_pixels(html));
    }

    #[test]
    fn remove_script_tags() {
        let html = "<p>Before</p><script>alert('xss')</script><p>After</p>";
        let cleaned = remove_unsafe_elements(html);
        assert!(!cleaned.contains("alert"));
        assert!(cleaned.contains("Before"));
        assert!(cleaned.contains("After"));
    }

    #[test]
    fn remove_style_tags() {
        let html = "<style>.foo{color:red}</style><p>Content</p>";
        let cleaned = remove_unsafe_elements(html);
        assert!(!cleaned.contains("color:red"));
        assert!(cleaned.contains("Content"));
    }

    #[test]
    fn remove_html_comments() {
        let html = "<p>A</p><!--[if mso]><table><tr><td>Outlook junk</td></tr></table><![endif]--><p>B</p>";
        let cleaned = remove_unsafe_elements(html);
        assert!(!cleaned.contains("Outlook junk"));
        assert!(cleaned.contains("A"));
        assert!(cleaned.contains("B"));
    }

    #[test]
    fn detect_bulk_sender_list_unsubscribe() {
        let headers = "From: news@example.com\r\nList-Unsubscribe: <mailto:unsub@example.com>\r\n";
        assert!(detect_bulk_sender(headers));
    }

    #[test]
    fn detect_bulk_sender_precedence() {
        let headers = "From: news@example.com\r\nPrecedence: bulk\r\n";
        assert!(detect_bulk_sender(headers));
    }

    #[test]
    fn not_bulk_sender_personal() {
        let headers = "From: alice@example.com\r\nTo: bob@example.com\r\n";
        assert!(!detect_bulk_sender(headers));
    }

    #[test]
    fn is_automated_sender_noreply() {
        assert!(is_automated_sender("noreply@example.com"));
        assert!(is_automated_sender("no-reply@example.com"));
        assert!(is_automated_sender("NOREPLY@Example.com"));
    }

    #[test]
    fn is_not_automated_sender() {
        assert!(!is_automated_sender("alice@example.com"));
        assert!(!is_automated_sender("support@example.com"));
    }

    #[test]
    fn split_quoted_on_wrote() {
        let text = "Thanks for the update.\n\nOn Mon, Jan 1, 2025 at 10:00 AM Alice <alice@x.com> wrote:\n> Original message";
        let (new_content, quoted) = split_quoted_content(text);
        assert_eq!(new_content, "Thanks for the update.");
        assert_eq!(quoted.len(), 1);
        assert!(quoted[0].contains("Alice"));
    }

    #[test]
    fn split_quoted_no_quote() {
        let text = "Just a simple message with no quotes.";
        let (new_content, quoted) = split_quoted_content(text);
        assert_eq!(new_content, text);
        assert!(quoted.is_empty());
    }

    #[test]
    fn split_quoted_outlook_style() {
        let text = "My reply.\n\nFrom: Bob\nSent: Monday\nTo: Alice\nSubject: Re: test\n\nOriginal";
        let (new_content, quoted) = split_quoted_content(text);
        assert_eq!(new_content, "My reply.");
        assert_eq!(quoted.len(), 1);
    }

    #[test]
    fn split_quoted_angle_bracket() {
        let text = "My reply.\n\n> line 1\n> line 2\n> line 3\n> line 4";
        let (new_content, quoted) = split_quoted_content(text);
        assert_eq!(new_content, "My reply.");
        assert_eq!(quoted.len(), 1);
    }

    #[test]
    fn text_to_html_ratio_pure_text() {
        let html = "Hello world";
        let result = clean_email_html(html);
        assert!(result.text_to_html_ratio > 0.5);
    }

    #[test]
    fn text_to_html_ratio_heavy_template() {
        let html = "<html><head><style>.a{}.b{}.c{}</style></head><body><table><tr><td><table><tr><td><img src='logo.png' width='600'></td></tr></table></td></tr><tr><td>Hi</td></tr><tr><td><a href='#'>Unsubscribe</a></td></tr></table></body></html>";
        let result = clean_email_html(html);
        // template-heavy emails have low ratio
        assert!(result.text_to_html_ratio < 0.3);
    }

    #[test]
    fn footer_removal() {
        let html = "<div>Important content here with lots of text that forms the main body of the email message.</div><div>More important content in the second paragraph.</div><div><a href='#'>unsubscribe</a> | <a href='#'>manage preferences</a></div>";
        let (result, removed) = remove_template_chrome(html);
        assert!(removed);
        assert!(result.contains("Important content"));
    }

    // ===== Additional corner-case tests =====

    #[test]
    fn detect_tracking_pixel_singled_quotes() {
        // tracking pixels often use single quotes — detection must cover both styles
        let html = "<img src='https://track.example.com/o/' width='1' height='1'>";
        assert!(detect_tracking_pixels(html));
    }

    #[test]
    fn detect_tracking_by_known_domain_even_full_size() {
        // A full-size image from a known tracking domain still counts as tracking.
        let html = r#"<img src="https://list-manage.com/wf/open?u=x" width="600" height="400">"#;
        assert!(detect_tracking_pixels(html));
    }

    #[test]
    fn detect_tracking_by_path_keyword() {
        let html = r#"<img src="https://example.com/track?id=1" width="600">"#;
        assert!(detect_tracking_pixels(html));
    }

    #[test]
    fn empty_html_returns_empty_result() {
        let result = clean_email_html("");
        assert_eq!(result.clean_text, "");
        assert!(!result.has_tracking_pixel);
        assert_eq!(result.link_count, 0);
        assert_eq!(result.image_count, 0);
        // ratio is 1.0 when html is empty
        assert!((result.text_to_html_ratio - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn detect_bulk_sender_auto_submitted_no_is_not_bulk() {
        // RFC 3834: auto-submitted: no means a human-sent reply, not bulk.
        let headers = "From: alice@example.com\r\nAuto-Submitted: no\r\n";
        assert!(!detect_bulk_sender(headers));
    }

    #[test]
    fn detect_bulk_sender_auto_submitted_yes() {
        // anything other than "no" is treated as auto / bulk
        let headers = "From: x@y\r\nAuto-Submitted: auto-replied\r\n";
        assert!(detect_bulk_sender(headers));
    }

    #[test]
    fn is_automated_sender_postmaster_bounce_variants() {
        assert!(is_automated_sender("postmaster@x.com"));
        assert!(is_automated_sender("mailer-daemon@x.com"));
        assert!(is_automated_sender("bounce-12345@x.com"));
        assert!(is_automated_sender("bounces@x.com"));
        assert!(is_automated_sender("notification-system@x.com"));
        assert!(is_automated_sender("notifications@x.com"));
        assert!(is_automated_sender("do-not-reply@x.com"));
        assert!(is_automated_sender("donotreply@x.com"));
        assert!(is_automated_sender("auto@x.com"));
        // Negative cases
        assert!(!is_automated_sender("alice@x.com"));
        assert!(!is_automated_sender("not-noreply@x.com"));  // doesn't match exact pattern
    }

    #[test]
    fn split_quoted_japanese_style_header() {
        let text = "私の返事です。\n\n2025年1月1日 10:00 Alice <alice@x.com>:\n> 元のメッセージ";
        let (new_content, quoted) = split_quoted_content(text);
        assert_eq!(new_content, "私の返事です。");
        assert_eq!(quoted.len(), 1);
    }

    #[test]
    fn split_quoted_short_block_of_quotes_not_split() {
        // Only 1-2 quoted lines is not enough to trigger split.
        let text = "Reply.\n\n> one quoted line\n\nAnother line.";
        let (new_content, _) = split_quoted_content(text);
        assert!(new_content.contains("Another line"));
    }

    #[test]
    fn count_pattern_case_insensitive() {
        // count_pattern lowercases — uppercase tags should still count
        let html = "<A href='1'>x</A><a href='2'>y</a>";
        assert_eq!(count_pattern(html, "<a "), 2);
    }

    #[test]
    fn footer_too_early_not_removed() {
        // If footer keyword appears in the first 40% of html, removal is suppressed
        // (otherwise we'd nuke most of the message).
        let html = "<div>unsubscribe</div><div>actual content here that constitutes most of the email body text</div>";
        let (result, removed) = remove_template_chrome(html);
        assert!(!removed, "removal suppressed when it would consume too much content");
        // unsubscribe word kept since not at the end
        assert!(result.contains("unsubscribe"));
    }
}

