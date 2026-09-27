//! What the fraud rules read out of a message body.
//!
//! Here for the same reason the header readers are in `identity`: the
//! receiver, the historical sweep and the offline checker each build
//! the rules' facts, and a second copy of "which parts count as the
//! text" would let one of them see a letter the others cannot.

use mailrs_fraud::advance_fee::{self, Body};

const HEAD_LIMIT: usize = 16 * 1024;

/// Whether the message's text offers its reader part of a sum in the
/// millions.
///
/// Every `text/plain` and `text/html` leaf that is not an attachment,
/// decoded by its own charset. Both alternatives of a
/// `multipart/alternative` are read; the check asks whether a phrase
/// is present, so reading the same words twice changes nothing.
pub fn offers_the_reader_a_sum(parsed: &mailrs_mime::Part<'_>) -> bool {
    let texts: Vec<(bool, String)> = parsed
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
        .collect();
    advance_fee::offers_the_reader_a_sum(texts.iter().map(|(html, t)| match html {
        true => Body::Html(t),
        false => Body::Plain(t),
    }))
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
    fn list_headers_mark_bulk_and_only_in_the_header_block() {
        assert!(is_bulk(
            b"From: a@b\r\nList-Unsubscribe: <mailto:u@b>\r\n\r\nbody"
        ));
        assert!(is_bulk(b"From: a@b\nLIST-ID: news.b\n\nbody"));
        assert!(!is_bulk(b"From: a@b\r\n\r\nList-Id: in the body\r\n"));
    }
}
