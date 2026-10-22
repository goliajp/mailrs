//! Text in a named charset, decoded the way mail in the wild needs.

use std::borrow::Cow;

use encoding_rs::{Encoding, ISO_2022_JP};

/// Decode `bytes` in the charset `label` names, or `None` when the label is
/// not one `encoding_rs` knows.
///
/// ISO-2022-JP from Japanese senders often switches charset and switches
/// again before any character (`ESC ( B ESC $ B`). The WHATWG decoder treats
/// that as an error and inserts U+FFFD; mail readers do not, so the switch
/// that is immediately superseded is dropped first.
pub fn decode_charset(label: &[u8], bytes: &[u8]) -> Option<String> {
    let encoding = Encoding::for_label(label)?;
    let bytes = match encoding == ISO_2022_JP {
        true => drop_superseded_escapes(bytes),
        false => Cow::Borrowed(bytes),
    };
    let (text, _, _) = encoding.decode(&bytes);
    Some(text.into_owned())
}

/// Length of the ISO-2022-JP escape sequence at the start of `rest`, if one
/// starts there: `ESC ( F`, `ESC $ F` or `ESC $ ( F`.
fn escape_len(rest: &[u8]) -> Option<usize> {
    match rest {
        [0x1b, b'$', b'(', _, ..] => Some(4),
        [0x1b, b'(' | b'$', _, ..] => Some(3),
        _ => None,
    }
}

fn drop_superseded_escapes(bytes: &[u8]) -> Cow<'_, [u8]> {
    let mut out: Option<Vec<u8>> = None;
    let mut i = 0;
    while i < bytes.len() {
        if let Some(len) = escape_len(&bytes[i..])
            && escape_len(&bytes[i + len..]).is_some()
        {
            out.get_or_insert_with(|| bytes[..i].to_vec());
            i += len;
            continue;
        }
        if let Some(o) = out.as_mut() {
            o.push(bytes[i]);
        }
        i += 1;
    }
    match out {
        Some(o) => Cow::Owned(o),
        None => Cow::Borrowed(bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::decode_charset;

    /// The bytes of a card-issuer notice: `ESC ( B ESC $ B` between `）`
    /// and `を` came out as `）�を`.
    #[test]
    fn a_switch_superseded_at_once_is_not_a_character() {
        let bytes = b"\x1b$B!J\x1b(BJCB\x1b$B!K\x1b(B\x1b$B$r\x1b(B";
        assert_eq!(
            decode_charset(b"iso-2022-jp", bytes).as_deref(),
            Some("\u{ff08}JCB\u{ff09}\u{3092}")
        );
    }

    #[test]
    fn other_charsets_and_unknown_labels() {
        assert_eq!(
            decode_charset(b"utf-8", "é".as_bytes()).as_deref(),
            Some("é")
        );
        assert_eq!(
            decode_charset(b"shift_jis", b"\x82\xa0").as_deref(),
            Some("\u{3042}")
        );
        assert_eq!(decode_charset(b"no-such-charset", b"x"), None);
    }
}
