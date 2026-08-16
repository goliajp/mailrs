//! The two header fields a reader uses to decide who a message is
//! from, read out of the raw message and checked for tampering.
//!
//! Defined once, here, because two lanes need the same answer and they
//! reach the message at different moments: the receiver has it in hand
//! during the SMTP transaction, and fastcore re-reads it from the
//! maildir when it stamps a verdict on mail that predates the field. A
//! second copy would let the badge on new mail disagree with the badge
//! on old mail, which is the failure this crate keeps having to fix.
//!
//! **From display name and Subject only.** Bodies are excluded on
//! purpose — see `mailrs_textguard`, which explains why, and which owns
//! the question of *which characters* deceive. This module owns only
//! *where to look*.

use mailrs_textguard::Deception;

/// How far into a message to look for headers. The header block of a
/// well-formed message is far smaller; a message whose From is past
/// 16 KB has bigger problems than a display name.
const HEAD_LIMIT: usize = 16 * 1024;

/// Read `From:` and `Subject:` out of a raw RFC 5322 message, decode
/// their encoded-words, and report which deceptive characters they
/// contain.
///
/// The decode is the part that is easy to leave out and fatal to leave
/// out: a display name written `=?UTF-8?B?…?=` hides its override
/// inside base64, and a check on the undecoded text sees only ASCII.
/// All five production examples arrived that way.
pub fn deception_in_identity(raw: &[u8]) -> Deception {
    let head = &raw[..raw.len().min(HEAD_LIMIT)];
    let text = String::from_utf8_lossy(head);
    let mut from = String::new();
    let mut subject = String::new();
    let mut field: Option<&mut String> = None;
    let mut pending = String::new();

    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        // A blank line ends the header block. Anything after it is body.
        if line.is_empty() {
            break;
        }
        // Continuation of the field before it (RFC 5322 folding). The
        // fold can fall inside an encoded-word, so join before decoding.
        if line.starts_with(' ') || line.starts_with('\t') {
            if field.is_some() {
                pending.push(' ');
                pending.push_str(line.trim());
            }
            continue;
        }
        if let Some(target) = field.take() {
            *target = mailrs_rfc2047::decode(pending.as_bytes()).into_owned();
        }
        pending.clear();
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("from:") {
            pending = line[line.len() - rest.len()..].trim().to_string();
            field = Some(&mut from);
        } else if let Some(rest) = lower.strip_prefix("subject:") {
            pending = line[line.len() - rest.len()..].trim().to_string();
            field = Some(&mut subject);
        }
    }
    if let Some(target) = field.take() {
        *target = mailrs_rfc2047::decode(pending.as_bytes()).into_owned();
    }

    mailrs_textguard::deception_in_any([from.as_str(), subject.as_str()])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The production message the user reported, header block verbatim
    /// apart from the base64, which is this display name encoded:
    /// a right-to-left override followed by `BCJyM`, which renders as
    /// `MyJCB`.
    #[test]
    fn the_reported_phish_reports_a_bidi_override() {
        let encoded = mailrs_rfc2047::encode("\u{202E}BCJyM");
        let raw = format!(
            "Return-Path: <alertpq43@wokjx.crabfishhh.com>\r\n\
             Authentication-Results: mail.golia.ai; spf=pass; dkim=pass; dmarc=pass\r\n\
             From: {encoded} <alertpq43@wokjx.crabfishhh.com>\r\n\
             Subject: =?UTF-8?B?44GU5Yip55So44GU56K66KqN?=\r\n\
             \r\n\
             body\r\n"
        );
        let d = deception_in_identity(raw.as_bytes());
        assert!(
            d.bidi_override,
            "the override in the From display name was not seen"
        );
    }

    /// **The decode is load-bearing.** Undecoded, the same message is
    /// pure ASCII and every check on it passes. This asserts the
    /// difference rather than trusting it.
    #[test]
    fn an_override_hidden_in_base64_is_still_found() {
        let encoded = mailrs_rfc2047::encode("\u{202E}BCJyM");
        assert!(
            encoded.is_ascii(),
            "premise: the encoded form hides the override in ASCII"
        );
        assert_eq!(
            mailrs_textguard::deception_in(&encoded),
            Deception::default(),
            "premise: the encoded form is clean until decoded"
        );
        let raw = format!("From: {encoded} <a@b.example>\r\n\r\nbody\r\n");
        assert!(deception_in_identity(raw.as_bytes()).bidi_override);
    }

    /// A folded Subject. The override is deliberately in the **second**
    /// segment: a continuation line that is dropped rather than joined
    /// takes the whole signal with it, and a first-segment override
    /// would pass this test either way.
    #[test]
    fn a_folded_subject_is_joined_before_decoding() {
        let raw = "From: A <a@b.example>\r\n\
                   Subject: =?UTF-8?B?SGVsbG8=?=\r\n \
                   =?UTF-8?B?4oCuQkNKeU0=?=\r\n\
                   \r\n\
                   body\r\n";
        assert!(
            deception_in_identity(raw.as_bytes()).bidi_override,
            "the continuation line was dropped and the override with it"
        );
    }

    /// Ordinary mail, including a Subject in a script that needs
    /// shaping and a body that would trip a body-scanning check.
    #[test]
    fn ordinary_mail_is_clean() {
        let raw = "From: Quora Digest <digest@quora.com>\r\n\
                   Subject: =?UTF-8?B?44GK55+l44KJ44Gb?=\r\n\
                   \r\n\
                   an invisible \u{200B} character in the body is not our business\r\n";
        assert_eq!(deception_in_identity(raw.as_bytes()), Deception::default());
    }

    /// Zero-width padding in a display name reports as the weaker
    /// signal, and separately — the caller scores it rather than
    /// convicting on it.
    #[test]
    fn zero_width_padding_reports_apart() {
        let encoded = mailrs_rfc2047::encode("M\u{200B}yJC\u{2060}B");
        let raw = format!("From: {encoded} <a@b.example>\r\n\r\nbody\r\n");
        assert_eq!(
            deception_in_identity(raw.as_bytes()),
            Deception {
                bidi_override: false,
                unjustified_zero_width: true
            }
        );
    }

    /// A message with no headers at all, and one that is empty. Neither
    /// is a crash and neither is a verdict.
    #[test]
    fn a_message_without_the_fields_is_clean() {
        assert_eq!(deception_in_identity(b""), Deception::default());
        assert_eq!(deception_in_identity(b"garbage\r\n"), Deception::default());
    }
}
