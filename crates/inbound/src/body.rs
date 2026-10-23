//! What the fraud rules read out of a message body.
//!
//! Here for the same reason the header readers are in `identity`: the
//! receiver, the historical sweep and the offline checker each build
//! the rules' facts, and a second copy of "which parts count as the
//! text" would let one of them see a letter the others cannot.

use mailrs_fraud::advance_fee::{self, Body};
use mailrs_fraud::line_handoff;

const HEAD_LIMIT: usize = 16 * 1024;

/// Whether the message's text offers its reader part of a sum in the
/// millions.
///
/// Every `text/plain` and `text/html` leaf that is not an attachment,
/// decoded by its own charset. Both alternatives of a
/// `multipart/alternative` are read; the check asks whether a phrase
/// is present, so reading the same words twice changes nothing.
pub fn offers_the_reader_a_sum(parsed: &mailrs_mime::Part<'_>) -> bool {
    let texts = texts(parsed);
    advance_fee::offers_the_reader_a_sum(bodies(&texts))
}

/// Whether the message's text asks its reader to send their own LINE
/// contact. The same parts as [`offers_the_reader_a_sum`].
pub fn asks_for_the_readers_line(parsed: &mailrs_mime::Part<'_>) -> bool {
    let texts = texts(parsed);
    line_handoff::asks_for_the_readers_line(bodies(&texts))
}

/// Every `text/plain` and `text/html` leaf that is not an attachment,
/// decoded, with whether it is HTML.
fn texts(parsed: &mailrs_mime::Part<'_>) -> Vec<(bool, String)> {
    parsed
        .walk()
        .filter(|p| p.children.is_empty() && !p.is_attachment())
        .filter(|p| p.content_type.type_ == "text")
        .filter_map(|p| {
            let html = match p.content_type.subtype.as_str() {
                "html" => true,
                "plain" => false,
                _ => return None,
            };
            p.body_text().map(|t| (html, t))
        })
        .collect()
}

fn bodies(texts: &[(bool, String)]) -> impl Iterator<Item = Body<'_>> {
    texts.iter().map(|(html, t)| match html {
        true => Body::Html(t),
        false => Body::Plain(t),
    })
}

/// [`offers_the_reader_a_sum`], over text extracted from attachments.
pub fn attachment_offers_the_reader_a_sum(text: &str) -> bool {
    !text.is_empty() && advance_fee::offers_the_reader_a_sum([Body::Plain(text)])
}

/// The attachments' texts as the rules read them: joined by blank lines
/// and cut at [`mailrs_fraud::ATTACHMENT_TEXT_CAP`] on a char boundary.
pub fn attachment_text_for_rules(texts: &[String]) -> String {
    let mut out = texts.join("\n\n");
    if out.len() > mailrs_fraud::ATTACHMENT_TEXT_CAP {
        let mut end = mailrs_fraud::ATTACHMENT_TEXT_CAP;
        while !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
    }
    out
}

/// Whether the message carries `List-Unsubscribe` or `List-Id`.
pub fn is_bulk(raw: &[u8]) -> bool {
    let head = &raw[..raw.len().min(HEAD_LIMIT)];
    let text = String::from_utf8_lossy(head);
    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        if line.is_empty() {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("list-unsubscribe:") || lower.starts_with("list-id:") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORTED: &[u8] = b"From: David Konczol <info@nexforce.in>\r\n\
To: undisclosed-recipients:;\r\n\
Subject: RE: PARTNERSHIP PROPOSITION\r\n\
Content-Type: text/plain; charset=UTF-8; format=flowed\r\n\
Content-Transfer-Encoding: 8bit\r\n\
\r\n\
in the sum of  Ten Million, Two \r\n\
Hundred Thousand United States Dollars only (US$10,200,000.00). we \r\n\
shall fill your Company's name as the Foreign Technical \r\n\
Partner to our deceased client. split equally, 50% apiece.\r\n";

    #[test]
    fn the_reported_message_reads_as_an_offer() {
        assert!(offers_the_reader_a_sum(&mailrs_mime::parse(REPORTED)));
        assert!(!is_bulk(REPORTED));
    }

    /// The HTML wave arrived quoted-printable; the words are only
    /// there after decoding.
    #[test]
    fn a_quoted_printable_html_part_is_decoded_first() {
        let raw = b"From: x <a@b.example>\r\n\
Content-Type: text/html\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\
\r\n\
<div>kickback money US=\r\n\
$25 million ... to partner with me in securing funds</div>\r\n";
        assert!(offers_the_reader_a_sum(&mailrs_mime::parse(raw)));
    }

    #[test]
    fn an_attachment_is_not_the_text() {
        let raw = b"From: x <a@b.example>\r\n\
Content-Type: multipart/mixed; boundary=b\r\n\
\r\n\
--b\r\n\
Content-Type: text/plain\r\n\
\r\n\
see attached\r\n\
--b\r\n\
Content-Type: text/plain; name=story.txt\r\n\
Content-Disposition: attachment; filename=story.txt\r\n\
\r\n\
$5 million, your share is half\r\n\
--b--\r\n";
        assert!(!offers_the_reader_a_sum(&mailrs_mime::parse(raw)));
    }

    #[test]
    fn an_offer_inside_an_attachment_is_read_by_the_attachment_check() {
        let text = "in the sum of US$10,200,000.00 ... split equally, 50% apiece";
        assert!(attachment_offers_the_reader_a_sum(text));
        assert!(!attachment_offers_the_reader_a_sum(
            "quarterly invoice, total 12,000 yen"
        ));
        assert!(!attachment_offers_the_reader_a_sum(""));
    }

    #[test]
    fn attachment_text_for_the_rules_stays_under_their_limit() {
        let texts = vec![
            "日".repeat(mailrs_fraud::ATTACHMENT_TEXT_CAP),
            "more".into(),
        ];
        let joined = attachment_text_for_rules(&texts);
        assert!(joined.len() <= mailrs_fraud::ATTACHMENT_TEXT_CAP);
        assert!(joined.len() > mailrs_fraud::ATTACHMENT_TEXT_CAP - 3);
        assert_eq!(
            attachment_text_for_rules(&["a".into(), "b".into()]),
            "a\n\nb"
        );
    }

    #[test]
    fn list_headers_mark_bulk_and_only_in_the_header_block() {
        assert!(is_bulk(
            b"From: a@b\r\nList-Unsubscribe: <mailto:u@b>\r\n\r\nbody"
        ));
        assert!(is_bulk(b"From: a@b\nLIST-ID: news.b\n\nbody"));
        assert!(!is_bulk(b"From: a@b\r\n\r\nList-Id: in the body\r\n"));
    }
}
