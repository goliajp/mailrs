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
    let (from, subject, ..) = decoded_identity(raw);
    mailrs_textguard::deception_in_any([from.as_str(), subject.as_str()])
}

/// The decoded `Subject:`.
///
/// From the same parse as [`from_header`], because a rule that reads
/// the subject and a rule that reads the name must be looking at the
/// same message — and because the subject arrives base64'd inside
/// `=?UTF-8?B?…?=` exactly as often as the name does.
pub fn subject_header(raw: &[u8]) -> String {
    decoded_identity(raw).1
}

/// The same reading, of the **display name alone**.
///
/// [`deception_in_identity`] folds the `From` and the `Subject`
/// together, which is right for the sender-trust verdict and wrong
/// for a rule about names: `mailrs_textguard` measured one legitimate
/// message in forty carrying a zero-width space, and it was in a
/// subject. Over 35,962 production messages, forty carry one inside
/// the display name and every one of the forty is a phish.
///
/// The address is excluded too — a zero-width character cannot
/// survive in one, and including it would only add ways to be wrong.
pub fn deception_in_display_name(raw: &[u8]) -> Deception {
    let from = decoded_identity(raw).0;
    let display = match from.rfind('<') {
        Some(open) => from[..open].trim().trim_matches('"').to_string(),
        None => String::new(),
    };
    mailrs_textguard::deception_in_any([display.as_str()])
}

/// The decoded `From:` value — display name and address together.
///
/// Separate from the deception check because two questions are asked of
/// the same two lines, and both need the **decoded** text: a name
/// arrives base64'd inside `=?UTF-8?B?…?=` in every real sample, and a
/// check on the raw header sees only ASCII.
pub fn from_header(raw: &[u8]) -> String {
    decoded_identity(raw).0
}

/// The message's `X-Mailer`, unfolded, when it carries one.
///
/// The **value**, not a verdict about it. It was
/// `mailer_looks_generated(raw) -> bool`, which folded the reading
/// and the rule into one function — so the rule could not be
/// rewritten, replaced or scripted without also rewriting the reader,
/// and no other rule could see the header at all.
///
/// Read here rather than by a stage, for the same reason `deception`
/// is: a property of the text, fixed before the pipeline starts, so
/// nothing about stage ordering can leave it unset.
///
/// **Continuation lines are joined** (RFC 5322 §2.2.3). The version
/// of this that did not cost a day on 2026-08-29: Exchange folds
/// `Message-ID:` onto the next line and the same reader missed it, so
/// a held conversation could not say why it was held. A folded
/// `X-Mailer` would be a check that silently does not fire, which is
/// worse — nothing would look wrong at all.
pub fn x_mailer_header(raw: &[u8]) -> Option<String> {
    let head = &raw[..raw.len().min(HEAD_LIMIT)];
    let text = String::from_utf8_lossy(head);
    let mut value: Option<String> = None;
    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        if line.is_empty() {
            break;
        }
        if let Some(v) = &mut value {
            match line.starts_with([' ', '\t']) {
                true => {
                    if !v.is_empty() {
                        v.push(' ');
                    }
                    v.push_str(line.trim());
                    continue;
                }
                false => break,
            }
        }
        if let Some(rest) = line.to_ascii_lowercase().strip_prefix("x-mailer:") {
            value = Some(line[line.len() - rest.len()..].trim().to_string());
        }
    }
    value.filter(|v| !v.is_empty())
}

/// The `To:` display name, decoded — the sender's own claim about
/// who this message is for.
///
/// Display name only: `李好 <lihao@golia.jp>` gives `李好`, and a
/// bare address gives the empty string. The address itself would
/// make a subject greeting `lihao` look answered, which is not what
/// is being asked.
pub fn to_display_name(raw: &[u8]) -> String {
    let to = decoded_identity(raw).2;
    match to.rfind('<') {
        Some(open) => to[..open].trim().trim_matches('"').to_string(),
        None => to.trim().trim_matches('"').to_string(),
    }
}

/// The bare address in `Reply-To:`, lowercased.
///
/// Empty when the message carries no `Reply-To`, which is most of
/// them — 36,318 production messages hold 265 distinct off-domain
/// reply addresses between them.
pub fn reply_to_address(raw: &[u8]) -> String {
    let v = decoded_identity(raw).3;
    let inner = match (v.rfind('<'), v.rfind('>')) {
        (Some(a), Some(b)) if a < b => &v[a + 1..b],
        _ => v.trim(),
    };
    inner.trim().to_ascii_lowercase()
}

/// `From:`, `Subject:`, `To:` and `Reply-To:`, decoded. One parser,
/// because a second copy is how the badge on new mail comes to
/// disagree with the badge on old mail — the failure this module
/// already exists to prevent.
fn decoded_identity(raw: &[u8]) -> (String, String, String, String) {
    let head = &raw[..raw.len().min(HEAD_LIMIT)];
    let text = String::from_utf8_lossy(head);
    let mut from = String::new();
    let mut subject = String::new();
    let mut to = String::new();
    let mut reply_to = String::new();
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
        } else if let Some(rest) = lower.strip_prefix("to:") {
            pending = line[line.len() - rest.len()..].trim().to_string();
            field = Some(&mut to);
        } else if let Some(rest) = lower.strip_prefix("reply-to:") {
            pending = line[line.len() - rest.len()..].trim().to_string();
            field = Some(&mut reply_to);
        }
    }
    if let Some(target) = field.take() {
        *target = mailrs_rfc2047::decode(pending.as_bytes()).into_owned();
    }

    (from, subject, to, reply_to)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LinkedIn's `To:` carries the reader's name, and it arrives
    /// encoded — 189 messages in the corpus, and the whole reason the
    /// greeting rule compares against this header rather than a list
    /// of names nobody can finish writing.
    #[test]
    fn the_to_display_name_is_decoded() {
        let raw = format!(
            "From: LinkedIn <x@linkedin.com>\r\nTo: {} <lihao@golia.jp>\r\n\r\nbody\r\n",
            mailrs_rfc2047::encode("李好")
        );
        assert_eq!(to_display_name(raw.as_bytes()), "李好");
    }

    /// The campaign's `To:` is the bare address, which carries no
    /// name at all — that is the contradiction with its subject.
    #[test]
    fn a_bare_address_carries_no_name() {
        let raw = b"From: x@shiye.airmessage.cn\r\nTo: lihao@golia.jp\r\n\r\nbody\r\n";
        assert_eq!(to_display_name(raw), "lihao@golia.jp");
    }

    /// Both forms the campaign uses, one of them folded — the shape
    /// that cost a day when `Message-ID` was read without unfolding.
    #[test]
    fn the_reply_address_is_read_in_either_form_and_when_folded() {
        for header in [
            "Reply-To: suqiqi@linghit.com",
            "Reply-To: <suqiqi@linghit.com>",
            "Reply-To: 苏七七\r\n <suqiqi@linghit.com>",
            "Reply-To: <SuQiQi@Linghit.COM>",
        ] {
            let raw = format!("From: x@shiye.airmessage.cn\r\n{header}\r\n\r\nbody\r\n");
            assert_eq!(
                reply_to_address(raw.as_bytes()),
                "suqiqi@linghit.com",
                "{header}"
            );
        }
    }

    #[test]
    fn no_reply_to_is_the_empty_string() {
        assert_eq!(reply_to_address(b"From: x@a.com\r\n\r\nbody\r\n"), "");
    }

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
                unjustified_zero_width: true,
                // The same name is also the narrower shape — every
                // one of its invisibles has a Latin letter on both
                // sides. That reading *is* conclusive, and this
                // fixture came from a phish, so it should be.
                zero_width_inside_a_word: true,
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
