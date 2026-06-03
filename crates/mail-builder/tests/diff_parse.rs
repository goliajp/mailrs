//! ckpt 2.2 — differential parse via proptest.
//!
//! Generate 1000 random valid `MessageBuilder` inputs; build each
//! one and parse the output with TWO independent MIME parsers
//! (`mailrs-mime` — our own — and `mail-parser` — third-party).
//! Assert structural equivalence: same part count, same MIME types
//! per part, same decoded body content per part. Disagreement
//! between two unrelated parsers is a strong signal that the
//! builder emitted something subtly wrong.

use mailrs_mail_builder::{Attachment, MessageBuilder};
use proptest::prelude::*;

/// What kind of payload to put in the message.
#[derive(Debug, Clone)]
enum BodyShape {
    TextOnly(String),
    HtmlOnly(String),
    TextPlusHtml(String, String),
    TextPlusAttachment(String, AttachmentSpec),
    TextPlusHtmlPlusAttachment(String, String, AttachmentSpec),
}

#[derive(Debug, Clone)]
struct AttachmentSpec {
    name: String,
    ct: String,
    data: Vec<u8>,
}

fn ascii_text() -> impl Strategy<Value = String> {
    // a few sentence-ish ASCII strings
    proptest::collection::vec("[a-zA-Z0-9 .,;:!?-]{1,40}", 1usize..6).prop_map(|v| v.join("\n"))
}

fn maybe_utf8_text() -> impl Strategy<Value = String> {
    prop_oneof![
        ascii_text(),
        Just("こんにちは世界".to_string()),
        Just("héllo wörld".to_string()),
        Just("emoji 🎉 test 🍕".to_string()),
        Just("Привет".to_string()),
    ]
}

fn html_text() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("<p>hello</p>".to_string()),
        Just("<html><body><h1>Title</h1><p>Body</p></body></html>".to_string()),
        Just("<div>Mixed <b>bold</b> and <i>italic</i></div>".to_string()),
    ]
}

fn attachment_strategy() -> impl Strategy<Value = AttachmentSpec> {
    // Use only NON-text content-types for randomly-generated binary
    // data. `mail-parser` decodes text/* parts as UTF-8 strings (and
    // replaces invalid sequences with U+FFFD) — random bytes labelled
    // text/* are a real misuse that one parser would tolerate and
    // another wouldn't. Either disagreement would be a builder bug,
    // but only with text/* + non-UTF-8 bytes, which we don't generate.
    (
        "[a-zA-Z][a-zA-Z0-9_-]{0,20}\\.(bin|pdf|jpg|gz|zip)",
        prop_oneof![
            Just("application/octet-stream".to_string()),
            Just("image/jpeg".to_string()),
            Just("application/pdf".to_string()),
            Just("application/gzip".to_string()),
            Just("application/zip".to_string()),
        ],
        proptest::collection::vec(any::<u8>(), 0..512),
    )
        .prop_map(|(name, ct, data)| AttachmentSpec { name, ct, data })
}

fn body_shape_strategy() -> impl Strategy<Value = BodyShape> {
    prop_oneof![
        maybe_utf8_text().prop_map(BodyShape::TextOnly),
        html_text().prop_map(BodyShape::HtmlOnly),
        (maybe_utf8_text(), html_text()).prop_map(|(t, h)| BodyShape::TextPlusHtml(t, h)),
        (maybe_utf8_text(), attachment_strategy())
            .prop_map(|(t, a)| BodyShape::TextPlusAttachment(t, a)),
        (maybe_utf8_text(), html_text(), attachment_strategy())
            .prop_map(|(t, h, a)| BodyShape::TextPlusHtmlPlusAttachment(t, h, a)),
    ]
}

fn build_from_shape(shape: &BodyShape) -> Vec<u8> {
    let mut b = MessageBuilder::new()
        .from("alice@example.com")
        .to("bob@example.com")
        .subject("diff_parse")
        .date("Wed, 27 May 2026 12:00:00 +0000")
        .message_id("<diff@example.com>");
    b =
        match shape {
            BodyShape::TextOnly(t) => b.text_body(t.clone()),
            BodyShape::HtmlOnly(h) => b.html_body(h.clone()),
            BodyShape::TextPlusHtml(t, h) => b.text_body(t.clone()).html_body(h.clone()),
            BodyShape::TextPlusAttachment(t, a) => b.text_body(t.clone()).attachment(
                Attachment::new(a.name.clone(), a.ct.clone(), a.data.clone()),
            ),
            BodyShape::TextPlusHtmlPlusAttachment(t, h, a) => b
                .text_body(t.clone())
                .html_body(h.clone())
                .attachment(Attachment::new(
                    a.name.clone(),
                    a.ct.clone(),
                    a.data.clone(),
                )),
        };
    b.build()
}

/// Flatten a `mailrs-mime` parse to a list of `(mime_type,
/// decoded_body_bytes)` leaves in DFS order. Multipart containers
/// are not included as leaves themselves.
fn flatten_mailrs(msg: &[u8]) -> Vec<(String, Vec<u8>)> {
    let part = mailrs_mime::part::parse(msg);
    let mut out = Vec::new();
    collect_mailrs(&part, &mut out);
    out
}

fn collect_mailrs(part: &mailrs_mime::part::Part<'_>, out: &mut Vec<(String, Vec<u8>)>) {
    if part.content_type.is_multipart() {
        for c in &part.children {
            collect_mailrs(c, out);
        }
    } else {
        // part.body is already decoded (Cow<[u8]> with CTE applied)
        out.push((part.content_type.mime_type(), part.body.to_vec()));
    }
}

/// Flatten a `mail-parser` parse to the same `(mime_type,
/// decoded_body_bytes)` list — same DFS order.
fn flatten_mail_parser(msg: &[u8]) -> Vec<(String, Vec<u8>)> {
    use mail_parser::{MessageParser, MimeHeaders, PartType};
    let parsed = MessageParser::default()
        .parse(msg)
        .expect("mail-parser parse");
    let mut out = Vec::new();
    for part in parsed.parts.iter() {
        match &part.body {
            PartType::Multipart(_) => continue, // skip multipart containers
            PartType::Text(cow) => {
                let ct = part
                    .content_type()
                    .map(|ct| {
                        let mut s = ct.ctype().to_string();
                        if let Some(sub) = ct.subtype() {
                            s.push('/');
                            s.push_str(sub);
                        }
                        s
                    })
                    .unwrap_or_else(|| "text/plain".to_string());
                out.push((ct, cow.as_bytes().to_vec()));
            }
            PartType::Html(cow) => {
                let ct = part
                    .content_type()
                    .map(|ct| {
                        let mut s = ct.ctype().to_string();
                        if let Some(sub) = ct.subtype() {
                            s.push('/');
                            s.push_str(sub);
                        }
                        s
                    })
                    .unwrap_or_else(|| "text/html".to_string());
                out.push((ct, cow.as_bytes().to_vec()));
            }
            PartType::Binary(bytes) | PartType::InlineBinary(bytes) => {
                let ct = part
                    .content_type()
                    .map(|ct| {
                        let mut s = ct.ctype().to_string();
                        if let Some(sub) = ct.subtype() {
                            s.push('/');
                            s.push_str(sub);
                        }
                        s
                    })
                    .unwrap_or_else(|| "application/octet-stream".to_string());
                out.push((ct, bytes.to_vec()));
            }
            PartType::Message(_) => {
                // message/rfc822 nested — not produced by 0.1 builder
                continue;
            }
        }
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1000,
        // disable shrinking-time runaway on large random attachments
        max_shrink_iters: 64,
        .. ProptestConfig::default()
    })]

    /// The two parsers must agree on (a) number of leaf parts,
    /// (b) MIME content-type per leaf, (c) decoded body bytes per
    /// leaf. Mismatch = builder emitted something subtly wrong.
    #[test]
    fn parsers_agree_on_built_message(shape in body_shape_strategy()) {
        let msg = build_from_shape(&shape);
        let mailrs = flatten_mailrs(&msg);
        let third = flatten_mail_parser(&msg);

        prop_assert_eq!(mailrs.len(), third.len(),
            "leaf-part count differs: mailrs={} mail-parser={}",
            mailrs.len(), third.len());

        for (i, ((mt_a, body_a), (mt_b, body_b))) in mailrs.iter().zip(third.iter()).enumerate() {
            // mime type strings agree (case-insensitive)
            prop_assert!(
                mt_a.eq_ignore_ascii_case(mt_b),
                "part {} mime type differs: mailrs={:?} mail-parser={:?}",
                i, mt_a, mt_b,
            );
            // decoded bodies byte-equal (after trimming trailing
            // CRLF that one parser may strip and the other keep)
            let trim = |b: &[u8]| -> Vec<u8> {
                let mut v = b.to_vec();
                while v.last() == Some(&b'\n') || v.last() == Some(&b'\r') {
                    v.pop();
                }
                v
            };
            prop_assert_eq!(
                trim(body_a), trim(body_b),
                "part {} decoded body differs", i,
            );
        }
    }
}
