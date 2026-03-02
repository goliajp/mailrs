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

impl Encoder<String> for ImapCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: String, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.extend_from_slice(item.as_bytes());
        Ok(())
    }
}
