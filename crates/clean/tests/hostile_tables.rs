//! Table markup that took the renderer down.
//!
//! The servers build with `panic = "abort"`, so a panic inside the HTML
//! renderer ends the whole process, not one request. `rowspan="0"` (valid
//! HTML: span to the end of the group) divided by zero in html2text before
//! 0.17.2, and a marketing mail carrying it stopped fastcore while it was
//! rescoring importance over a copy of production.

const ROWSPAN_ZERO: &str = "<html><body><table>\
<tr><td rowspan=\"0\">left</td><td>one</td></tr>\
<tr><td>two</td></tr>\
</table></body></html>";

#[test]
fn a_zero_rowspan_is_rendered_not_a_crash() {
    let cleaned = mailrs_clean::clean_email_html(ROWSPAN_ZERO);
    assert!(
        cleaned.clean_text.contains("left"),
        "{:?}",
        cleaned.clean_text
    );
}

#[test]
fn a_zero_rowspan_in_the_search_text_is_rendered_not_a_crash() {
    let raw = format!("Content-Type: text/html; charset=utf-8\r\n\r\n{ROWSPAN_ZERO}\r\n");
    let root = mailrs_mime::parse(raw.as_bytes());
    let text = mailrs_clean::body_text_for_search(&root).expect("a body");
    assert!(text.contains("one"), "{text:?}");
}
