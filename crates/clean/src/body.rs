//! The words of a message body, as a reader sees them.

use crate::css_text::strip_css_rules;

/// The text of a message: its first `text/plain` part, else its first
/// `text/html` part rendered as text (style sheets, scripts and comments
/// dropped), else the root part's text. Parts that are attachments are not
/// the body and are skipped. Each part is decoded by its transfer encoding
/// and charset.
///
/// Two things a sender's plain part sometimes carries are not text and are
/// taken out: HTML source sent as plain text (see [`is_html_source`]) is
/// rendered like an HTML part, and CSS rule blocks left in the text (see
/// [`strip_css_rules`]) are removed. `None` when there is no text at all,
/// a bare attachment say.
pub fn body_text_for_search(root: &mailrs_mime::Part<'_>) -> Option<String> {
    let mut html: Option<String> = None;
    for part in root.walk().filter(|p| !p.is_attachment()) {
        match part.content_type.mime_type().as_str() {
            "text/plain" => {
                if let Some(t) = part.body_text() {
                    return Some(plain_text(t));
                }
            }
            "text/html" if html.is_none() => html = part.body_text(),
            _ => {}
        }
    }
    if let Some(h) = html {
        return Some(html_text(h));
    }
    if root.is_attachment() {
        return None;
    }
    root.body_text().map(plain_text)
}

fn plain_text(t: String) -> String {
    if is_html_source(&t) {
        return html_text(t);
    }
    strip_css_rules(&t)
}

fn html_text(h: String) -> String {
    let text = html2text::from_read(h.as_bytes(), 100).unwrap_or(h);
    strip_css_rules(&text)
}

/// Elements whose tags, counted, say a plain part is HTML source. Layout
/// and document elements only: a person writing about HTML in a letter
/// mentions one or two, a template carries dozens.
const HTML_SOURCE_ELEMENTS: [&str; 14] = [
    "html", "head", "body", "style", "table", "tbody", "tr", "td", "th", "div", "br", "p", "span",
    "font",
];

/// How many such tags make a plain part HTML source.
const HTML_SOURCE_MIN_TAGS: usize = 8;

/// Whether `text` is HTML source: at least `HTML_SOURCE_MIN_TAGS` tags of
/// `HTML_SOURCE_ELEMENTS`, opening or closing, each written `<name`
/// (or `</name`) followed by `>`, `/` or whitespace, case ignored.
pub fn is_html_source(text: &str) -> bool {
    let b = text.as_bytes();
    let mut n = 0;
    let mut i = 0;
    while let Some(off) = memchr::memchr(b'<', &b[i..]) {
        let start = i + off + 1;
        let name_at = start + usize::from(b.get(start) == Some(&b'/'));
        let len = b[name_at..]
            .iter()
            .take_while(|c| c.is_ascii_alphabetic())
            .count();
        let ends_tag = matches!(
            b.get(name_at + len),
            Some(b'>' | b'/' | b' ' | b'\t' | b'\r' | b'\n')
        );
        if len > 0 && ends_tag {
            let name = &text[name_at..name_at + len];
            if HTML_SOURCE_ELEMENTS
                .iter()
                .any(|e| e.eq_ignore_ascii_case(name))
            {
                n += 1;
                if n >= HTML_SOURCE_MIN_TAGS {
                    return true;
                }
            }
        }
        i = start;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_wins_over_html() {
        let raw = b"Content-Type: multipart/alternative; boundary=x\r\n\r\n\
--x\r\nContent-Type: text/html\r\n\r\n<p>from html</p>\r\n\
--x\r\nContent-Type: text/plain\r\n\r\nfrom plain\r\n--x--\r\n";
        let text = body_text_for_search(&mailrs_mime::parse(raw)).unwrap();
        assert_eq!(text.trim(), "from plain");
    }

    #[test]
    fn html_loses_its_style_sheet_script_and_comments() {
        let raw = b"Content-Type: text/html\r\n\r\n<html><head><style>td{color:inherit}</style>\
</head><body><!-- note --><script>var x=1;</script><p>visible</p></body></html>\r\n";
        let text = body_text_for_search(&mailrs_mime::parse(raw)).unwrap();
        assert_eq!(text.trim(), "visible");
    }

    #[test]
    fn an_attached_text_file_is_not_the_body() {
        let raw = b"Content-Type: multipart/mixed; boundary=x\r\n\r\n\
--x\r\nContent-Type: text/plain; name=log.txt\r\n\
Content-Disposition: attachment; filename=log.txt\r\n\r\nlog line\r\n--x--\r\n";
        assert_eq!(body_text_for_search(&mailrs_mime::parse(raw)), None);
    }

    /// A template's plain part that is its HTML source, style sheet and all.
    #[test]
    fn html_source_sent_as_plain_text_is_rendered() {
        let raw = b"Content-Type: multipart/alternative; boundary=x\r\n\r\n\
--x\r\nContent-Type: text/plain\r\n\r\n<html><head><style>table{border-collapse:collapse}</style>\
</head><body><table><tr><td style=\"border-spacing:0\">Your parcel</td></tr>\
<tr><td><div>has shipped</div><br></td></tr></table></body></html>\r\n\
--x\r\nContent-Type: text/html\r\n\r\n<p>ignored</p>\r\n--x--\r\n";
        let text = body_text_for_search(&mailrs_mime::parse(raw)).unwrap();
        assert!(
            text.contains("Your parcel") && text.contains("has shipped"),
            "{text}"
        );
        assert!(!text.contains("border"), "{text}");
        assert!(!text.contains('<'), "{text}");
    }

    /// Someone writing about HTML is not sending it.
    #[test]
    fn a_letter_mentioning_a_few_tags_stays_plain() {
        let t = "Wrap it in a <div> and add a <br> after the <p>, please.";
        assert!(!is_html_source(t));
        let raw = format!("Content-Type: text/plain\r\n\r\n{t}\r\n");
        let text = body_text_for_search(&mailrs_mime::parse(raw.as_bytes())).unwrap();
        assert_eq!(text.trim(), t);
    }

    #[test]
    fn the_tag_count_is_of_layout_elements_only() {
        let many_links = "<a href=x>1</a> <a href=y>2</a> <b>3</b> <i>4</i> <em>5</em> <a>6</a> <a>7</a> <a>8</a>";
        assert!(!is_html_source(many_links));
        let rows = "<tr><td>a</td><td>b</td></tr><tr><td>c</td></tr>";
        assert!(is_html_source(rows), "8 tags of rows and cells");
        assert!(!is_html_source(
            "<trx> <tdz> <divine> <brand> <pre> <ps> <bodyguard> <tables>"
        ));
    }

    /// A style sheet left in a plain part by a sender's own conversion.
    #[test]
    fn css_rules_in_plain_text_are_removed() {
        let raw = b"Content-Type: text/plain\r\n\r\nHello\r\n\
table { border-collapse: collapse; border-spacing: 0 } td{mso-line-height-rule:exactly;}\r\n\
@media only screen and (max-width:600px) { .wrap { width: 100% !important; } }\r\n\
See you\r\n";
        let text = body_text_for_search(&mailrs_mime::parse(raw)).unwrap();
        for css in [
            "max-width",
            "border-collapse",
            "border-spacing",
            "mso-line",
            "100%",
            "important",
        ] {
            assert!(!text.contains(css), "{css} left in {text}");
        }
        assert!(text.contains("Hello") && text.contains("See you"), "{text}");
    }
}
