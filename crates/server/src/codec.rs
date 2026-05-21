use bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::config::SmuggleProtection;

/// maximum command line length (RFC 5321 section 4.5.3.1.4: 512 octets including CRLF)
const MAX_COMMAND_LINE: usize = 1024;

/// SMTP codec that switches between command mode and DATA mode.
/// in command mode: reads lines terminated by CRLF.
/// in data mode: reads raw bytes until ".\r\n" terminator.
pub struct SmtpCodec {
    data_mode: bool,
    max_message_size: usize,
    smuggle_protection: SmuggleProtection,
}

impl SmtpCodec {
    pub fn new() -> Self {
        Self {
            data_mode: false,
            max_message_size: mailrs_smtp_proto::session::MAX_MESSAGE_SIZE as usize,
            smuggle_protection: SmuggleProtection::Permissive,
        }
    }

    pub fn with_smuggle_protection(mut self, mode: SmuggleProtection) -> Self {
        self.smuggle_protection = mode;
        self
    }

    pub fn enter_data_mode(&mut self) {
        self.data_mode = true;
    }
}

#[derive(Debug)]
pub enum SmtpInput {
    Command(String),
    Data(Vec<u8>),
    DataRejected,
}

impl Decoder for SmtpCodec {
    type Item = SmtpInput;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if self.data_mode {
            // reject oversized messages mid-stream
            if src.len() > self.max_message_size {
                src.clear();
                self.data_mode = false;
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "message exceeds maximum size",
                ));
            }
            // look for the terminator: CRLF.CRLF ("\r\n.\r\n")
            if let Some(pos) = find_data_terminator(src) {
                let data = src.split_to(pos + 3).to_vec(); // include ".\r\n"
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
            // reject oversized command lines before looking for CRLF
            if src.len() > MAX_COMMAND_LINE && find_crlf(src).is_none() {
                src.clear();
                return Ok(Some(SmtpInput::Command(String::new())));
            }
            // look for CRLF
            if let Some(pos) = find_crlf(src) {
                let line = src.split_to(pos);
                src.advance(2); // skip CRLF
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
    // looking for "\r\n.\r\n" — return position of the '.'
    buf.windows(5)
        .position(|w| w == b"\r\n.\r\n")
        .map(|p| p + 2) // point to '.' so we include ".\r\n"
}

/// detect SMTP smuggling sequences (bare LF before dot-terminator)
pub fn has_smuggle_sequence(data: &[u8]) -> Option<usize> {
    for i in 0..data.len().saturating_sub(2) {
        if data[i] == b'\n' && (i == 0 || data[i - 1] != b'\r') && data[i + 1] == b'.' {
            // bare LF followed by dot + line ending
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

/// normalize line endings: bare LF → CRLF, bare CR → CRLF
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
}
