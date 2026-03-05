use bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

/// IMAP codec with line mode and literal data mode
pub struct ImapCodec {
    literal_remaining: Option<u32>,
}

#[derive(Debug)]
pub enum ImapInput {
    Line(String),
    LiteralData(Vec<u8>),
}

impl ImapCodec {
    pub fn new() -> Self {
        Self {
            literal_remaining: None,
        }
    }

    /// enter literal mode to read N bytes of data
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
                // consume trailing CRLF if present
                if src.len() >= 2 && &src[..2] == b"\r\n" {
                    src.advance(2);
                }
                return Ok(Some(ImapInput::LiteralData(data)));
            }
            return Ok(None);
        }

        if let Some(pos) = src.windows(2).position(|w| w == b"\r\n") {
            let line = src.split_to(pos);
            src.advance(2); // skip CRLF
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

    // helper: create a codec and decode from raw bytes
    fn decode_once(codec: &mut ImapCodec, data: &[u8]) -> Result<Option<ImapInput>, std::io::Error> {
        let mut buf = BytesMut::from(data);
        codec.decode(&mut buf)
    }

    // --- line parsing ---

    #[test]
    fn decode_simple_line() {
        let mut codec = ImapCodec::new();
        let mut buf = BytesMut::from("A001 LOGIN user pass\r\n");
        let result = codec.decode(&mut buf).unwrap();
        match result {
            Some(ImapInput::Line(s)) => assert_eq!(s, "A001 LOGIN user pass"),
            other => panic!("expected Line, got {:?}", other),
        }
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_empty_line() {
        let mut codec = ImapCodec::new();
        let mut buf = BytesMut::from("\r\n");
        let result = codec.decode(&mut buf).unwrap();
        match result {
            Some(ImapInput::Line(s)) => assert_eq!(s, ""),
            other => panic!("expected empty Line, got {:?}", other),
        }
    }

    #[test]
    fn decode_incomplete_line_returns_none() {
        let mut codec = ImapCodec::new();
        let result = decode_once(&mut codec, b"A001 NOOP");
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn decode_line_with_bare_lf_not_matched() {
        // only CRLF terminates a line, bare LF should not
        let mut codec = ImapCodec::new();
        let result = decode_once(&mut codec, b"A001 NOOP\n");
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn decode_two_lines_sequentially() {
        let mut codec = ImapCodec::new();
        let mut buf = BytesMut::from("A001 NOOP\r\nA002 LOGOUT\r\n");

        let first = codec.decode(&mut buf).unwrap();
        match first {
            Some(ImapInput::Line(s)) => assert_eq!(s, "A001 NOOP"),
            other => panic!("expected first Line, got {:?}", other),
        }

        let second = codec.decode(&mut buf).unwrap();
        match second {
            Some(ImapInput::Line(s)) => assert_eq!(s, "A002 LOGOUT"),
            other => panic!("expected second Line, got {:?}", other),
        }

        assert!(buf.is_empty());
    }

    #[test]
    fn decode_line_preserves_internal_crlf_like_content() {
        // data containing \r not followed by \n should not split
        let mut codec = ImapCodec::new();
        let mut buf = BytesMut::from("hello\rworld\r\n");
        let result = codec.decode(&mut buf).unwrap();
        match result {
            Some(ImapInput::Line(s)) => assert_eq!(s, "hello\rworld"),
            other => panic!("expected Line, got {:?}", other),
        }
    }

    // --- literal handling ---

    #[test]
    fn decode_literal_exact_size() {
        let mut codec = ImapCodec::new();
        codec.expect_literal(5);
        let mut buf = BytesMut::from("ABCDE\r\n");
        let result = codec.decode(&mut buf).unwrap();
        match result {
            Some(ImapInput::LiteralData(data)) => assert_eq!(data, b"ABCDE"),
            other => panic!("expected LiteralData, got {:?}", other),
        }
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_literal_without_trailing_crlf() {
        let mut codec = ImapCodec::new();
        codec.expect_literal(3);
        let mut buf = BytesMut::from("ABCnext");
        let result = codec.decode(&mut buf).unwrap();
        match result {
            Some(ImapInput::LiteralData(data)) => assert_eq!(data, b"ABC"),
            other => panic!("expected LiteralData, got {:?}", other),
        }
        // "next" should remain in buffer
        assert_eq!(&buf[..], b"next");
    }

    #[test]
    fn decode_literal_incomplete_returns_none() {
        let mut codec = ImapCodec::new();
        codec.expect_literal(10);
        let result = decode_once(&mut codec, b"short");
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn decode_literal_zero_length() {
        let mut codec = ImapCodec::new();
        codec.expect_literal(0);
        let mut buf = BytesMut::from("\r\n");
        let result = codec.decode(&mut buf).unwrap();
        match result {
            Some(ImapInput::LiteralData(data)) => assert!(data.is_empty()),
            other => panic!("expected empty LiteralData, got {:?}", other),
        }
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_literal_then_line() {
        let mut codec = ImapCodec::new();
        codec.expect_literal(4);
        let mut buf = BytesMut::from("data\r\nA003 OK\r\n");

        // first decode: literal
        let lit = codec.decode(&mut buf).unwrap();
        match lit {
            Some(ImapInput::LiteralData(d)) => assert_eq!(d, b"data"),
            other => panic!("expected LiteralData, got {:?}", other),
        }

        // second decode: line (literal mode cleared)
        let line = codec.decode(&mut buf).unwrap();
        match line {
            Some(ImapInput::Line(s)) => assert_eq!(s, "A003 OK"),
            other => panic!("expected Line, got {:?}", other),
        }
    }

    #[test]
    fn decode_literal_containing_crlf() {
        // literal data can contain CRLF bytes - they should not split
        let mut codec = ImapCodec::new();
        codec.expect_literal(6);
        let mut buf = BytesMut::from("AB\r\nCD\r\n");
        let result = codec.decode(&mut buf).unwrap();
        match result {
            Some(ImapInput::LiteralData(data)) => assert_eq!(data, b"AB\r\nCD"),
            other => panic!("expected LiteralData, got {:?}", other),
        }
        // trailing \r\n consumed
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

        let result = codec.decode(&mut buf).unwrap();
        match result {
            Some(ImapInput::LiteralData(data)) => assert_eq!(data, binary),
            other => panic!("expected LiteralData, got {:?}", other),
        }
    }

    // --- encoder ---

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

    // --- state transitions ---

    #[test]
    fn new_codec_has_no_literal_pending() {
        let mut codec = ImapCodec::new();
        // should decode as line mode
        let mut buf = BytesMut::from("test\r\n");
        let result = codec.decode(&mut buf).unwrap();
        assert!(matches!(result, Some(ImapInput::Line(_))));
    }

    #[test]
    fn expect_literal_clears_after_read() {
        let mut codec = ImapCodec::new();
        codec.expect_literal(2);
        let mut buf = BytesMut::from("OK\r\nA001 DONE\r\n");

        // consume literal
        let _ = codec.decode(&mut buf).unwrap();
        // next decode should be line mode
        let result = codec.decode(&mut buf).unwrap();
        match result {
            Some(ImapInput::Line(s)) => assert_eq!(s, "A001 DONE"),
            other => panic!("expected Line after literal, got {:?}", other),
        }
    }

    #[test]
    fn decode_non_utf8_line_uses_lossy_conversion() {
        let mut codec = ImapCodec::new();
        let mut buf = BytesMut::from(&b"hello \xff world\r\n"[..]);
        let result = codec.decode(&mut buf).unwrap();
        match result {
            Some(ImapInput::Line(s)) => {
                assert!(s.contains("hello"));
                assert!(s.contains("world"));
                // invalid byte replaced with U+FFFD
                assert!(s.contains('\u{FFFD}'));
            }
            other => panic!("expected Line, got {:?}", other),
        }
    }

    #[test]
    fn decode_partial_crlf_at_buffer_end() {
        // buffer ends with \r but no \n — should not match
        let mut codec = ImapCodec::new();
        let result = decode_once(&mut codec, b"A001 NOOP\r");
        assert!(result.unwrap().is_none());
    }
}
