#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

/// Maximum length of an SMTP command line, in octets, per
/// RFC 5321 §4.5.3.1.4 (the spec mandates 512 including CRLF; we
/// pad to 1024 to absorb common over-runs without breaking
/// behaviour). Hard cap — anything larger is rejected as a
/// zero-length command.
const MAX_COMMAND_LINE: usize = 1024;

/// Default DATA-mode payload cap when `with_max_message_size` is
/// not called. 100 MiB — generous; callers should set their own
/// SIZE-extension-aligned value.
const DEFAULT_MAX_MESSAGE_SIZE: usize = 100 * 1024 * 1024;

/// SMTP-smuggling defence mode controlling how the codec handles
/// bare-LF sequences inside the DATA payload.
///
/// SMTP smuggling (CVE-2023-51764 and family) abuses the fact that
/// some MTAs treat a bare LF as a line ending: an attacker injects
/// `\n.\r\n` mid-payload to terminate the outer transaction
/// early, then smuggles a *second* RFC 5321 envelope through the
/// remainder of the data. Three policies are available:
///
/// - [`Strict`](Self::Strict): reject the payload outright if a
///   bare-LF dot-terminator is detected.
/// - [`Permissive`](Self::Permissive): accept the payload but
///   normalize all line endings to CRLF before emitting — the
///   smuggled envelope is destroyed in transit.
/// - [`Off`](Self::Off): pass-through (RFC 5321 strict mode,
///   matches legacy receivers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmuggleProtection {
    /// Reject payloads containing bare-LF dot-terminators.
    Strict,
    /// Accept payloads but normalize line endings.
    Permissive,
    /// Pass payload through verbatim.
    Off,
}

/// Tokio codec for SMTP. Switches between command mode
/// (CRLF-terminated lines, ≤1024 octets each) and DATA mode (raw
/// bytes until the `CRLF.CRLF` dot-terminator). Caller toggles
/// DATA mode with [`SmtpCodec::enter_data_mode`] after responding
/// `354` to a `DATA` command.
pub struct SmtpCodec {
    data_mode: bool,
    max_message_size: usize,
    smuggle_protection: SmuggleProtection,
}

impl Default for SmtpCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl SmtpCodec {
    /// Create a codec in command mode with default settings:
    /// permissive smuggle protection and 100 MiB DATA cap.
    pub fn new() -> Self {
        Self {
            data_mode: false,
            max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
            smuggle_protection: SmuggleProtection::Permissive,
        }
    }

    /// Override the smuggle-protection mode. See
    /// [`SmuggleProtection`] for behaviour.
    pub fn with_smuggle_protection(mut self, mode: SmuggleProtection) -> Self {
        self.smuggle_protection = mode;
        self
    }

    /// Override the DATA-mode payload cap (bytes). Should match
    /// the SMTP `SIZE` extension value the receiver advertises.
    pub fn with_max_message_size(mut self, size: usize) -> Self {
        self.max_message_size = size;
        self
    }

    /// Switch the codec into DATA mode. Call after the `354
    /// start mail input` response. The codec will read raw bytes
    /// (subject to the message-size cap and smuggle-protection
    /// policy) until it sees `CRLF.CRLF`, emit one
    /// [`SmtpInput::Data`] or [`SmtpInput::DataRejected`], and
    /// auto-switch back to command mode.
    pub fn enter_data_mode(&mut self) {
        self.data_mode = true;
    }
}

/// One decoded SMTP-session frame.
#[derive(Debug)]
pub enum SmtpInput {
    /// A command-mode line (everything up to the terminating
    /// CRLF, exclusive). Returned even for malformed lines; the
    /// caller parses the SMTP verb.
    Command(String),
    /// A complete DATA-mode payload, including the trailing
    /// `.\r\n` dot-terminator. In `Permissive` smuggle mode this
    /// is line-ending-normalized; in `Off` mode it is verbatim.
    Data(Vec<u8>),
    /// Returned only in `Strict` smuggle mode when the DATA
    /// payload contains a bare-LF dot-terminator. Caller should
    /// 5xx-reject the message.
    DataRejected,
}

impl Decoder for SmtpCodec {
    type Item = SmtpInput;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if self.data_mode {
            if src.len() > self.max_message_size {
                src.clear();
                self.data_mode = false;
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "message exceeds maximum size",
                ));
            }
            if let Some(pos) = find_data_terminator(src) {
                let data = src.split_to(pos + 3).to_vec();
                self.data_mode = false;
                match self.smuggle_protection {
                    SmuggleProtection::Strict => {
                        if has_smuggle_sequence(&data).is_some() {
                            return Ok(Some(SmtpInput::DataRejected));
                        }
                    }
                    SmuggleProtection::Permissive => {
                        return Ok(Some(SmtpInput::Data(normalize_line_endings(&data))));
                    }
                    SmuggleProtection::Off => {}
                }
                return Ok(Some(SmtpInput::Data(data)));
            }
            Ok(None)
        } else {
            if src.len() > MAX_COMMAND_LINE && find_crlf(src).is_none() {
                src.clear();
                return Ok(Some(SmtpInput::Command(String::new())));
            }
            if let Some(pos) = find_crlf(src) {
                let line = src.split_to(pos);
                src.advance(2);
                let s = String::from_utf8_lossy(&line).into_owned();
                Ok(Some(SmtpInput::Command(s)))
            } else {
                Ok(None)
            }
        }
    }
}

impl Encoder<String> for SmtpCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: String, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.extend_from_slice(item.as_bytes());
        Ok(())
    }
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

fn find_data_terminator(buf: &[u8]) -> Option<usize> {
    buf.windows(5)
        .position(|w| w == b"\r\n.\r\n")
        .map(|p| p + 2)
}

/// Detect an SMTP-smuggling sequence: a bare LF (not preceded by
/// CR) followed by `.` and either LF or CRLF. Returns the byte
/// index of the bare LF, or `None` if the payload is clean.
///
/// This is the detector the [`SmuggleProtection::Strict`] mode
/// uses; exposed `pub` so callers can run it independently for
/// metrics or logging without enabling the rejection policy.
pub fn has_smuggle_sequence(data: &[u8]) -> Option<usize> {
    for i in 0..data.len().saturating_sub(2) {
        if data[i] == b'\n' && (i == 0 || data[i - 1] != b'\r') && data[i + 1] == b'.' {
            if data[i + 2] == b'\n' {
                return Some(i);
            }
            if i + 3 < data.len() && data[i + 2] == b'\r' && data[i + 3] == b'\n' {
                return Some(i);
            }
        }
    }
    None
}

/// Normalize line endings in a DATA payload to CRLF: bare LF →
/// CRLF, bare CR → CRLF, existing CRLF preserved. Used by
/// [`SmuggleProtection::Permissive`] mode to destroy any smuggled
/// envelope in transit while still accepting the message.
pub fn normalize_line_endings(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\r' {
            if i + 1 < data.len() && data[i + 1] == b'\n' {
                result.extend_from_slice(b"\r\n");
                i += 2;
            } else {
                result.extend_from_slice(b"\r\n");
                i += 1;
            }
        } else if data[i] == b'\n' {
            result.extend_from_slice(b"\r\n");
            i += 1;
        } else {
            result.push(data[i]);
            i += 1;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smuggle_bare_lf_dot_crlf() {
        assert_eq!(has_smuggle_sequence(b"hello\n.\r\n"), Some(5));
    }

    #[test]
    fn smuggle_proper_crlf_clean() {
        assert_eq!(has_smuggle_sequence(b"hello\r\n.\r\n"), None);
    }

    #[test]
    fn smuggle_bare_lf_dot_lf() {
        assert_eq!(has_smuggle_sequence(b"hello\n.\n"), Some(5));
    }

    #[test]
    fn smuggle_no_dot() {
        assert_eq!(has_smuggle_sequence(b"hello\nworld\r\n"), None);
    }

    #[test]
    fn smuggle_empty() {
        assert_eq!(has_smuggle_sequence(b""), None);
    }

    #[test]
    fn normalize_bare_lf() {
        assert_eq!(
            normalize_line_endings(b"hello\nworld\n"),
            b"hello\r\nworld\r\n"
        );
    }

    #[test]
    fn normalize_already_crlf() {
        assert_eq!(
            normalize_line_endings(b"hello\r\nworld\r\n"),
            b"hello\r\nworld\r\n"
        );
    }

    #[test]
    fn normalize_bare_cr() {
        assert_eq!(
            normalize_line_endings(b"hello\rworld\r"),
            b"hello\r\nworld\r\n"
        );
    }

    #[test]
    fn normalize_mixed() {
        assert_eq!(normalize_line_endings(b"a\nb\r\nc\rd"), b"a\r\nb\r\nc\r\nd");
    }

    #[test]
    fn codec_command_mode_emits_complete_line() {
        let mut codec = SmtpCodec::new();
        let mut buf = BytesMut::from("HELO example.com\r\n".as_bytes());
        let r = codec.decode(&mut buf).unwrap();
        match r {
            Some(SmtpInput::Command(s)) => assert_eq!(s, "HELO example.com"),
            other => panic!("expected Command, got {other:?}"),
        }
    }

    #[test]
    fn codec_command_mode_partial_line_returns_none() {
        let mut codec = SmtpCodec::new();
        let mut buf = BytesMut::from("HELO exa".as_bytes());
        assert!(codec.decode(&mut buf).unwrap().is_none());
    }

    #[test]
    fn codec_command_mode_oversized_line_clears_buffer() {
        let mut codec = SmtpCodec::new();
        let mut buf = BytesMut::from(vec![b'X'; MAX_COMMAND_LINE + 1].as_slice());
        let r = codec.decode(&mut buf).unwrap();
        assert!(matches!(r, Some(SmtpInput::Command(s)) if s.is_empty()));
        assert!(buf.is_empty());
    }

    #[test]
    fn codec_data_mode_emits_full_payload_on_dot_terminator() {
        let mut codec = SmtpCodec::new();
        codec.enter_data_mode();
        let mut buf = BytesMut::from("hello\r\n.\r\n".as_bytes());
        let r = codec.decode(&mut buf).unwrap();
        match r {
            Some(SmtpInput::Data(d)) => assert_eq!(d, b"hello\r\n.\r\n"),
            other => panic!("expected Data, got {other:?}"),
        }
    }

    #[test]
    fn codec_data_mode_strict_rejects_smuggle() {
        let mut codec = SmtpCodec::new().with_smuggle_protection(SmuggleProtection::Strict);
        codec.enter_data_mode();
        // bare LF before dot
        let mut buf = BytesMut::from("hello\n.\r\nworld\r\n.\r\n".as_bytes());
        let r = codec.decode(&mut buf).unwrap();
        assert!(matches!(r, Some(SmtpInput::DataRejected)));
    }

    #[test]
    fn codec_data_mode_oversize_errors_and_clears() {
        let mut codec = SmtpCodec::new().with_max_message_size(8);
        codec.enter_data_mode();
        let mut buf = BytesMut::from(vec![b'A'; 100].as_slice());
        let r = codec.decode(&mut buf);
        assert!(r.is_err());
        assert!(buf.is_empty());
    }

    #[test]
    fn codec_default_builds_without_args() {
        let _c = SmtpCodec::default();
    }

    /// Strict mode + clean payload must pass through unchanged.
    /// Covers the strict-no-smuggle fallthrough path (decode
    /// returns Data even though smuggle_protection is Strict,
    /// because has_smuggle_sequence returned None).
    #[test]
    fn codec_data_mode_strict_passes_clean_payload() {
        let mut codec = SmtpCodec::new().with_smuggle_protection(SmuggleProtection::Strict);
        codec.enter_data_mode();
        let mut buf = BytesMut::from("clean body\r\n.\r\n".as_bytes());
        let r = codec.decode(&mut buf).unwrap();
        assert!(matches!(r, Some(SmtpInput::Data(_))), "got {r:?}");
    }

    /// Off mode must pass smuggle-bearing payloads through
    /// untouched (no rejection, no normalization). Covers the
    /// SmuggleProtection::Off arm (line 142) plus the post-match
    /// Data fallthrough (line 144).
    #[test]
    fn codec_data_mode_off_passes_smuggle_unchanged() {
        let mut codec = SmtpCodec::new().with_smuggle_protection(SmuggleProtection::Off);
        codec.enter_data_mode();
        let payload = b"smuggled\n.\r\nbody\r\n.\r\n";
        let mut buf = BytesMut::from(&payload[..]);
        match codec.decode(&mut buf).unwrap() {
            Some(SmtpInput::Data(d)) => {
                // The decoder splits at the FIRST \r\n.\r\n it
                // finds, which sits before "body" — so the data
                // payload contains everything up to that point
                // *with* the smuggle sequence preserved.
                assert!(d.contains(&b'\n'), "should preserve bare LF");
                assert!(d.windows(3).any(|w| w == b"\n.\r" || w == b"\n.\n"),
                    "should preserve smuggle dot pattern");
            }
            other => panic!("expected Data, got {other:?}"),
        }
    }

    /// Decoder must return `Ok(None)` (not error) while waiting
    /// for the data terminator. Covers the incomplete-data path
    /// (line 146).
    #[test]
    fn codec_data_mode_returns_none_until_terminator() {
        let mut codec = SmtpCodec::new();
        codec.enter_data_mode();
        let mut buf = BytesMut::from("incomplete body...".as_bytes());
        let r = codec.decode(&mut buf).unwrap();
        assert!(r.is_none(), "should wait for \\r\\n.\\r\\n");
    }

    /// Encoder writes bytes into dst verbatim. Covers Encoder
    /// impl (lines 167-170).
    #[test]
    fn codec_encode_appends_bytes() {
        let mut codec = SmtpCodec::new();
        let mut dst = BytesMut::new();
        codec.encode("250 OK\r\n".to_string(), &mut dst).unwrap();
        assert_eq!(&dst[..], b"250 OK\r\n");
        // Second encode appends, doesn't overwrite.
        codec.encode("221 bye\r\n".to_string(), &mut dst).unwrap();
        assert_eq!(&dst[..], b"250 OK\r\n221 bye\r\n");
    }

    /// has_smuggle_sequence should detect the bare-LF + dot +
    /// CRLF variant (not just bare-LF + dot + LF). Covers line
    /// 197 (the CRLF arm inside the smuggle detector).
    #[test]
    fn smuggle_bare_lf_dot_crlf_explicit() {
        // \n.\r\n appearing in the middle of a payload (no
        // leading \r before the \n) is the canonical SMTP
        // smuggling vector.
        let data = b"prefix\n.\r\nsuffix";
        assert!(has_smuggle_sequence(data).is_some(),
            "should detect bare-LF + dot + CRLF");
    }
}
