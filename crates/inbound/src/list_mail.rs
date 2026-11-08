//! Whether a message is list mail: it carries `List-Unsubscribe` (RFC
//! 2369, RFC 8058) or `List-Id` (RFC 2919).
//!
//! On its own this proves nothing, since anyone can write the header. It
//! means something beside a DMARC pass, which proves the `From:` domain
//! sent the message: authenticated bulk mail from a domain that offers an
//! unsubscribe link is a newsletter or a notification, and a reader who
//! does not want it unsubscribes. See [`crate::make_delivery_decision`].

/// The header block carries a `List-Unsubscribe:` or `List-Id:` field.
///
/// Only field names at the start of a line in the header block count: a
/// folded continuation line starts with whitespace, and the body is
/// never read.
pub fn has_list_headers(raw: &[u8]) -> bool {
    for line in raw.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            return false;
        }
        let named =
            |name: &[u8]| line.len() > name.len() && line[..name.len()].eq_ignore_ascii_case(name);
        if named(b"list-unsubscribe:") || named(b"list-id:") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn either_header_makes_list_mail() {
        assert!(has_list_headers(
            b"From: a@b.example\r\nList-Unsubscribe: <https://b.example/u>\r\n\r\nhi"
        ));
        assert!(has_list_headers(
            b"From: a@b.example\r\nlist-id: News <news.b.example>\r\n\r\nhi"
        ));
    }

    #[test]
    fn a_header_named_in_the_body_or_a_folded_line_does_not() {
        assert!(!has_list_headers(
            b"From: a@b.example\r\n\r\nList-Unsubscribe: <https://b.example/u>\r\n"
        ));
        assert!(!has_list_headers(
            b"Subject: about\r\n List-Id: folded into the subject\r\n\r\nhi"
        ));
        assert!(!has_list_headers(
            b"From: a@b.example\nList-Post: <x>\n\nhi"
        ));
    }
}
