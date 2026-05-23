#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

/// Tokio codec for IMAP. Switches between line mode (CRLF-terminated
/// commands + responses) and literal mode (raw byte-counted payloads).
///
/// IMAP uses literals (e.g. `{12}\r\nHello world!`) for arbitrary
/// binary content — passwords with special chars, APPEND payloads,
/// FETCH BODY[…] data. The protocol layer parses the `{N}` marker,
/// then calls [`expect_literal`](Self::expect_literal) to tell the
/// codec to read the next N bytes as raw data instead of splitting
/// on CRLF.
pub struct ImapCodec {
    literal_remaining: Option<u32>,
}

/// One decoded IMAP-session frame.
#[derive(Debug)]
pub enum ImapInput {
    /// A line-mode frame (everything up to the CRLF, exclusive).
    /// Returned for both client commands (`A001 LOGIN …`) and
    /// continuation requests (`+ ready for literal`). Non-UTF-8
    /// bytes are replaced with U+FFFD (lossy conversion).
    Line(String),
    /// A literal-mode frame: exactly N bytes as requested by the
    /// most recent [`ImapCodec::expect_literal`] call. The trailing
    /// CRLF that often follows a literal is consumed automatically
    /// when present.
    LiteralData(Vec<u8>),
}

impl Default for ImapCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl ImapCodec {
    /// New codec in line mode.
    pub fn new() -> Self {
        Self {
            literal_remaining: None,
        }
    }

    /// Switch the codec into literal mode for the next decode.
    /// `size` is the byte count parsed from the IMAP `{N}` marker.
    /// After exactly `size` bytes have been read, the codec
    /// auto-switches back to line mode (consuming any trailing
    /// CRLF if present).
    pub fn expect_literal(&mut self, size: u32) {
        self.literal_remaining = Some(size);
    }
}

impl Decoder for ImapCodec {
    type Item = ImapInput;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(remaining) = self.literal_remaining {
            let needed = remaining as usize;
            if src.len() >= needed {
                let data = src.split_to(needed).to_vec();
                self.literal_remaining = None;
                if src.len() >= 2 && &src[..2] == b"\r\n" {
                    src.advance(2);
                }
                return Ok(Some(ImapInput::LiteralData(data)));
            }
            return Ok(None);
        }

        if let Some(pos) = src.windows(2).position(|w| w == b"\r\n") {
            let line = src.split_to(pos);
            src.advance(2);
            let s = String::from_utf8_lossy(&line).into_owned();
            Ok(Some(ImapInput::Line(s)))
        } else {
            Ok(None)
        }
    }
}

impl Encoder<Vec<u8>> for ImapCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: Vec<u8>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.extend_from_slice(&item);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use tokio_util::codec::{Decoder, Encoder};

    fn decode_once(
        codec: &mut ImapCodec,
        data: &[u8],
    ) -> Result<Option<ImapInput>, std::io::Error> {
        let mut buf = BytesMut::from(data);
        codec.decode(&mut buf)
    }

    #[test]
    fn decode_simple_line() {
        let mut codec = ImapCodec::new();
        let mut buf = BytesMut::from("A001 LOGIN user pass\r\n");
        let result = codec.decode(&mut buf).unwrap();
        match result {
            Some(ImapInput::Line(s)) => assert_eq!(s, "A001 LOGIN user pass"),
            other => panic!("expected Line, got {other:?}"),
        }
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_empty_line() {
        let mut codec = ImapCodec::new();
        let mut buf = BytesMut::from("\r\n");
        match codec.decode(&mut buf).unwrap() {
            Some(ImapInput::Line(s)) => assert_eq!(s, ""),
            other => panic!("expected empty Line, got {other:?}"),
        }
    }

    #[test]
    fn decode_incomplete_line_returns_none() {
        let mut codec = ImapCodec::new();
        assert!(decode_once(&mut codec, b"A001 NOOP").unwrap().is_none());
    }

    #[test]
    fn decode_line_with_bare_lf_not_matched() {
        let mut codec = ImapCodec::new();
        assert!(decode_once(&mut codec, b"A001 NOOP\n").unwrap().is_none());
    }

    #[test]
    fn decode_two_lines_sequentially() {
        let mut codec = ImapCodec::new();
        let mut buf = BytesMut::from("A001 NOOP\r\nA002 LOGOUT\r\n");
        match codec.decode(&mut buf).unwrap() {
            Some(ImapInput::Line(s)) => assert_eq!(s, "A001 NOOP"),
            other => panic!("first: {other:?}"),
        }
        match codec.decode(&mut buf).unwrap() {
            Some(ImapInput::Line(s)) => assert_eq!(s, "A002 LOGOUT"),
            other => panic!("second: {other:?}"),
        }
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_line_preserves_internal_cr_when_no_lf() {
        let mut codec = ImapCodec::new();
        let mut buf = BytesMut::from("hello\rworld\r\n");
        match codec.decode(&mut buf).unwrap() {
            Some(ImapInput::Line(s)) => assert_eq!(s, "hello\rworld"),
            other => panic!("expected Line, got {other:?}"),
        }
    }

    #[test]
    fn decode_literal_exact_size() {
        let mut codec = ImapCodec::new();
        codec.expect_literal(5);
        let mut buf = BytesMut::from("ABCDE\r\n");
        match codec.decode(&mut buf).unwrap() {
            Some(ImapInput::LiteralData(data)) => assert_eq!(data, b"ABCDE"),
            other => panic!("expected LiteralData, got {other:?}"),
        }
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_literal_without_trailing_crlf() {
        let mut codec = ImapCodec::new();
        codec.expect_literal(3);
        let mut buf = BytesMut::from("ABCnext");
        match codec.decode(&mut buf).unwrap() {
            Some(ImapInput::LiteralData(data)) => assert_eq!(data, b"ABC"),
            other => panic!("expected LiteralData, got {other:?}"),
        }
        assert_eq!(&buf[..], b"next");
    }

    #[test]
    fn decode_literal_incomplete_returns_none() {
        let mut codec = ImapCodec::new();
        codec.expect_literal(10);
        assert!(decode_once(&mut codec, b"short").unwrap().is_none());
    }

    #[test]
    fn decode_literal_zero_length() {
        let mut codec = ImapCodec::new();
        codec.expect_literal(0);
        let mut buf = BytesMut::from("\r\n");
        match codec.decode(&mut buf).unwrap() {
            Some(ImapInput::LiteralData(data)) => assert!(data.is_empty()),
            other => panic!("expected empty LiteralData, got {other:?}"),
        }
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_literal_then_line() {
        let mut codec = ImapCodec::new();
        codec.expect_literal(4);
        let mut buf = BytesMut::from("data\r\nA003 OK\r\n");
        match codec.decode(&mut buf).unwrap() {
            Some(ImapInput::LiteralData(d)) => assert_eq!(d, b"data"),
            other => panic!("expected LiteralData, got {other:?}"),
        }
        match codec.decode(&mut buf).unwrap() {
            Some(ImapInput::Line(s)) => assert_eq!(s, "A003 OK"),
            other => panic!("expected Line, got {other:?}"),
        }
    }

    #[test]
    fn decode_literal_containing_crlf() {
        let mut codec = ImapCodec::new();
        codec.expect_literal(6);
        let mut buf = BytesMut::from("AB\r\nCD\r\n");
        match codec.decode(&mut buf).unwrap() {
            Some(ImapInput::LiteralData(data)) => assert_eq!(data, b"AB\r\nCD"),
            other => panic!("expected LiteralData, got {other:?}"),
        }
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_literal_with_binary_data() {
        let mut codec = ImapCodec::new();
        let binary: Vec<u8> = (0u8..=255).collect();
        let size = binary.len() as u32;
        codec.expect_literal(size);
        let mut buf = BytesMut::from(binary.as_slice());
        buf.extend_from_slice(b"\r\n");
        match codec.decode(&mut buf).unwrap() {
            Some(ImapInput::LiteralData(data)) => assert_eq!(data, binary),
            other => panic!("expected LiteralData, got {other:?}"),
        }
    }

    #[test]
    fn encode_copies_bytes_to_dst() {
        let mut codec = ImapCodec::new();
        let mut dst = BytesMut::new();
        codec.encode(b"* OK ready\r\n".to_vec(), &mut dst).unwrap();
        assert_eq!(&dst[..], b"* OK ready\r\n");
    }

    #[test]
    fn encode_appends_to_existing_buffer() {
        let mut codec = ImapCodec::new();
        let mut dst = BytesMut::from("existing");
        codec.encode(b"+more".to_vec(), &mut dst).unwrap();
        assert_eq!(&dst[..], b"existing+more");
    }

    #[test]
    fn encode_empty_vec() {
        let mut codec = ImapCodec::new();
        let mut dst = BytesMut::new();
        codec.encode(vec![], &mut dst).unwrap();
        assert!(dst.is_empty());
    }

    #[test]
    fn new_codec_has_no_literal_pending() {
        let mut codec = ImapCodec::new();
        let mut buf = BytesMut::from("test\r\n");
        assert!(matches!(
            codec.decode(&mut buf).unwrap(),
            Some(ImapInput::Line(_))
        ));
    }

    #[test]
    fn expect_literal_clears_after_read() {
        let mut codec = ImapCodec::new();
        codec.expect_literal(2);
        let mut buf = BytesMut::from("OK\r\nA001 DONE\r\n");
        let _ = codec.decode(&mut buf).unwrap();
        match codec.decode(&mut buf).unwrap() {
            Some(ImapInput::Line(s)) => assert_eq!(s, "A001 DONE"),
            other => panic!("expected Line after literal, got {other:?}"),
        }
    }

    #[test]
    fn decode_non_utf8_line_uses_lossy_conversion() {
        let mut codec = ImapCodec::new();
        let mut buf = BytesMut::from(&b"hello \xff world\r\n"[..]);
        match codec.decode(&mut buf).unwrap() {
            Some(ImapInput::Line(s)) => {
                assert!(s.contains("hello"));
                assert!(s.contains("world"));
                assert!(s.contains('\u{FFFD}'));
            }
            other => panic!("expected Line, got {other:?}"),
        }
    }

    #[test]
    fn decode_partial_crlf_at_buffer_end() {
        let mut codec = ImapCodec::new();
        assert!(decode_once(&mut codec, b"A001 NOOP\r").unwrap().is_none());
    }

    #[test]
    fn default_constructs_same_as_new() {
        let _c = ImapCodec::default();
    }
}
