//! The words of a message body, as a reader sees them.

/// The text of a message: its first `text/plain` part, else its first
/// `text/html` part rendered as text (style sheets, scripts and comments
/// dropped), else the root part's text. Parts that are attachments are not
/// the body and are skipped. Each part is decoded by its transfer encoding
/// and charset. `None` when there is no text at all, a bare attachment say.
pub fn body_text_for_search(root: &mailrs_mime::Part<'_>) -> Option<String> {
    let mut html: Option<String> = None;
    for part in root.walk().filter(|p| !p.is_attachment()) {
        match part.content_type.mime_type().as_str() {
            "text/plain" => {
                if let Some(t) = part.body_text() {
                    return Some(t);
                }
            }
            "text/html" if html.is_none() => html = part.body_text(),
            _ => {}
        }
    }
    if let Some(h) = html {
        return Some(html2text::from_read(h.as_bytes(), 100).unwrap_or(h));
    }
    if root.is_attachment() {
        return None;
    }
    root.body_text()
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
}
