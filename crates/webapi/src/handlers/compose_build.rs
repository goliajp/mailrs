//! The bytes of an outgoing message, from the parts a compose collected.

use super::compose::{
    Attachment, ComposeParts, base64_wrap, from_header, random_hex, rfc5322_date,
};
use crate::handlers::prefs::now_secs;

/// Whether a value [`build_rfc5322`] writes into a header as it is carries
/// a line break. One would end that header and start another of the
/// sender's choosing (a Bcc, a second body), so every send path refuses
/// such a compose before building it.
pub(crate) fn breaks_a_header_line(parts: &ComposeParts) -> bool {
    let attachment_values = parts
        .attachments
        .iter()
        .flat_map(|a| std::iter::once(&a.content_type).chain(a.content_id.as_ref()));
    parts
        .to
        .iter()
        .chain(&parts.cc)
        .chain(&parts.bcc)
        .chain([&parts.from, &parts.subject])
        .chain(parts.in_reply_to.as_ref())
        .chain(attachment_values)
        .any(|v| v.contains(['\r', '\n']))
}

/// Build a complete RFC 5322 envelope from parsed compose parts.
///
/// Transport-safe by construction:
/// - subject: RFC 2047 encoded-word
/// - attachment filenames: RFC 2231 (`filename*=UTF-8''<pct>`) so
///   non-ASCII filenames survive strict MTAs and older MUAs
/// - text/plain + text/html bodies: base64 (safe on non-8BITMIME hops,
///   avoids quoted-printable blow-up on CJK)
/// - attachment payload: base64 with 76-column line wrapping
///
/// The shape, each layer only when it has something to hold:
///
/// ```text
/// multipart/mixed            files
/// └ multipart/related        images the HTML shows through cid:
///   └ multipart/alternative  text/plain + text/html
/// ```
///
/// An image with a Content-ID goes beside the HTML only when there is
/// HTML to show it; a plain-text message carries it as a file.
pub(crate) fn build_rfc5322(parts: &ComposeParts, from: &str) -> (String, Vec<u8>) {
    let mid_local = random_hex(8);
    let mid_host = mailrs_rfc5322::addr_spec(from)
        .split('@')
        .nth(1)
        .unwrap_or("localhost");
    let message_id = format!("{mid_local}@{mid_host}");
    let date = rfc5322_date(now_secs());
    let has_html = !parts.html_body.is_empty();
    let (shown, files): (Vec<&Attachment>, Vec<&Attachment>) = parts
        .attachments
        .iter()
        .partition(|a| has_html && a.content_id.is_some());

    let mut out = String::new();
    out.push_str(&format!("Date: {date}\r\n"));
    out.push_str(&format!("From: {}\r\n", from_header(from)));
    if !parts.to.is_empty() {
        out.push_str(&format!("To: {}\r\n", parts.to.join(", ")));
    }
    if !parts.cc.is_empty() {
        out.push_str(&format!("Cc: {}\r\n", parts.cc.join(", ")));
    }
    let encoded_subject = mailrs_rfc2047::encode(&parts.subject);
    out.push_str(&format!("Subject: {encoded_subject}\r\n"));
    out.push_str(&format!("Message-ID: <{message_id}>\r\n"));
    out.push_str("MIME-Version: 1.0\r\n");
    if let Some(ref irt) = parts.in_reply_to {
        out.push_str(&format!("In-Reply-To: <{irt}>\r\n"));
        out.push_str(&format!("References: <{irt}>\r\n"));
    }

    let mut entity = text_entity(parts);
    if !shown.is_empty() {
        entity = multipart("related", entity, &shown);
    }
    if !files.is_empty() {
        entity = multipart("mixed", entity, &files);
    }
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(&entity);
    (message_id, bytes)
}

/// The body: text/plain alone, or text/plain and text/html as
/// alternatives. Headers and content, as one MIME entity.
fn text_entity(parts: &ComposeParts) -> Vec<u8> {
    let mut s = String::new();
    if parts.html_body.is_empty() {
        s.push_str("Content-Type: text/plain; charset=utf-8\r\n");
        s.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
        s.push_str(&base64_wrap(parts.body.as_bytes()));
        return s.into_bytes();
    }
    let alt = format!("----=Alt_{}", random_hex(6));
    s.push_str(&format!(
        "Content-Type: multipart/alternative; boundary=\"{alt}\"\r\n\r\n"
    ));
    s.push_str(&format!("--{alt}\r\n"));
    s.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    s.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
    s.push_str(&base64_wrap(parts.body.as_bytes()));
    s.push_str(&format!("\r\n--{alt}\r\n"));
    s.push_str("Content-Type: text/html; charset=utf-8\r\n");
    s.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
    s.push_str(&base64_wrap(parts.html_body.as_bytes()));
    s.push_str(&format!("\r\n--{alt}--\r\n"));
    s.into_bytes()
}

/// `first` followed by `rest`, wrapped in a `multipart/<subtype>`.
fn multipart(subtype: &str, first: Vec<u8>, rest: &[&Attachment]) -> Vec<u8> {
    let boundary = format!("----=Part_{}", random_hex(6));
    let mut out = format!(
        "Content-Type: multipart/{subtype}; boundary=\"{boundary}\"\r\n\r\n--{boundary}\r\n"
    )
    .into_bytes();
    out.extend_from_slice(&first);
    for att in rest {
        out.extend_from_slice(part(&boundary, att, subtype == "related").as_bytes());
    }
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}

/// One part. An image the HTML shows carries no file name: readers count
/// a named part as a file, and this one is part of the body.
fn part(boundary: &str, att: &Attachment, shown: bool) -> String {
    let mut p = format!("\r\n--{boundary}\r\n");
    match (shown, &att.content_id) {
        (true, Some(cid)) => {
            p.push_str(&format!("Content-Type: {}\r\n", att.content_type));
            p.push_str(&format!("Content-ID: <{cid}>\r\n"));
            p.push_str("Content-Disposition: inline\r\n");
        }
        _ => {
            let ct_name = mailrs_rfc2231::encode_param("name", &att.filename);
            let cd_name = mailrs_rfc2231::encode_param("filename", &att.filename);
            p.push_str(&format!(
                "Content-Type: {ct}; {ct_name}\r\n",
                ct = att.content_type,
            ));
            p.push_str(&format!("Content-Disposition: attachment; {cd_name}\r\n"));
        }
    }
    p.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
    p.push_str(&base64_wrap(&att.bytes));
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(html: &str, attachments: Vec<Attachment>) -> ComposeParts {
        ComposeParts {
            from: "me@example.test".into(),
            to: vec!["you@example.test".into()],
            cc: vec![],
            bcc: vec![],
            subject: "Pictures".into(),
            body: "see below".into(),
            html_body: html.into(),
            in_reply_to: None,
            forward_message_id: None,
            forward_attachments_from: None,
            reply_to_thread_id: None,
            attachments,
            scheduled_at: None,
        }
    }

    fn att(name: &str, cid: Option<&str>) -> Attachment {
        Attachment {
            filename: name.into(),
            content_type: "image/png".into(),
            bytes: vec![0x89, b'P', b'N', b'G'],
            content_id: cid.map(String::from),
        }
    }

    /// A reader finds the image under the Content-ID the HTML names, as an
    /// inline part, and the file as a file.
    #[test]
    fn an_image_the_html_shows_travels_with_its_content_id() {
        let p = parts(
            "<p><img src=\"cid:pic1@inline\"></p>",
            vec![att("pic.png", Some("pic1@inline")), att("report.png", None)],
        );
        let (_, bytes) = build_rfc5322(&p, "me@example.test");
        let parsed = mailrs_mime::parse(&bytes);
        let files: Vec<_> = parsed.files().collect();
        let shown = files
            .iter()
            .find(|f| f.content_id.as_deref() == Some("pic1@inline"))
            .expect("the image travels under its Content-ID");
        assert!(
            !shown
                .disposition
                .as_ref()
                .is_some_and(|d| d.is_attachment())
        );
        assert_eq!(parsed.attachments().count(), 1, "only the report is a file");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("multipart/related"), "{text}");
        assert!(text.contains("multipart/mixed"), "{text}");
    }

    #[test]
    fn a_line_break_in_any_header_value_is_found() {
        assert!(!breaks_a_header_line(&parts(
            "",
            vec![att("a.png", Some("c@x"))]
        )));
        let edits: [fn(&mut ComposeParts); 7] = [
            |p| p.to.push("a@x\r\nBcc: e@x".into()),
            |p| p.cc.push("a@x\nBcc: e@x".into()),
            |p| p.bcc.push("a@x\rBcc: e@x".into()),
            |p| p.subject.push_str("\r\nBcc: e@x"),
            |p| p.in_reply_to = Some("m@x>\r\nBcc: <e@x".into()),
            |p| p.from.push_str("\nBcc: e@x"),
            |p| p.attachments[0].content_type.push_str("\r\nX: 1"),
        ];
        for (i, edit) in edits.iter().enumerate() {
            let mut p = parts("", vec![att("a.png", None)]);
            edit(&mut p);
            assert!(breaks_a_header_line(&p), "case {i}");
        }
    }

    #[test]
    fn without_html_an_image_with_a_content_id_is_a_file() {
        let p = parts("", vec![att("pic.png", Some("pic1@inline"))]);
        let (_, bytes) = build_rfc5322(&p, "me@example.test");
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("multipart/related"), "{text}");
        assert_eq!(mailrs_mime::parse(&bytes).attachments().count(), 1);
    }
}
