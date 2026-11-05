//! The frames between a client and `mailrs-parse-worker`.
//!
//! Every frame is `[u32 LE length][u8 kind][body]`, the length counting
//! the kind byte and the body. The client sends one [`REQUEST`] and reads
//! [`PROGRESS`] frames until an [`ANSWER`] or [`ERROR`]: a progress frame
//! names the parser about to run, so a worker that dies says by dying
//! which parser it died in.
//!
//! A request body is `[u32 LE header length][header JSON][input bytes]`;
//! an answer body is `[u32 LE header length][header JSON]` followed by
//! `[u32 LE length][bytes]` for each blob the answer carries (report
//! payloads, which need not be text).

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

pub const REQUEST: u8 = 0;
pub const PROGRESS: u8 = 1;
pub const ANSWER: u8 = 2;
pub const ERROR: u8 = 3;

/// The largest frame either side accepts. A message is at most the
/// receiver's size limit; the rest is headroom for the header.
pub const MAX_FRAME: usize = 256 * 1024 * 1024;

/// What to read out of the input bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// A whole message.
    Message(Want),
    /// A message's report attachments (DMARC, TLS-RPT), decompressed.
    ReportPayloads,
    /// The text of one Office file.
    Office { kind: OfficeKind },
    /// The embedded text of one PDF, page by page.
    Pdf,
}

/// The readings of [`Op::Message`] a caller wants.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Want {
    /// [`mailrs_clean::body_text_for_search`].
    pub body_text: bool,
    /// [`mailrs_fraud::attachment_inside::read`] over the attachments.
    pub inside: bool,
    /// Bayes tokens by this tokenizer.
    pub tokens: Option<Tokenizer>,
    /// What the first HTML part's markup says, for importance scoring.
    pub html_facts: bool,
}

/// The two Bayes tokenizers. Which corpus counts which is the triage
/// crate's to say; this only names the functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tokenizer {
    /// [`mailrs_bayes::tokenize_raw`].
    Raw,
    /// [`mailrs_bayes::tokenize`], over decoded text.
    Decoded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfficeKind {
    Word,
    Excel,
    PowerPoint,
}

impl From<mailrs_attachment_extract::office::OfficeKind> for OfficeKind {
    fn from(k: mailrs_attachment_extract::office::OfficeKind) -> Self {
        use mailrs_attachment_extract::office::OfficeKind as K;
        match k {
            K::Word => Self::Word,
            K::Excel => Self::Excel,
            K::PowerPoint => Self::PowerPoint,
        }
    }
}

impl From<OfficeKind> for mailrs_attachment_extract::office::OfficeKind {
    fn from(k: OfficeKind) -> Self {
        match k {
            OfficeKind::Word => Self::Word,
            OfficeKind::Excel => Self::Excel,
            OfficeKind::PowerPoint => Self::PowerPoint,
        }
    }
}

/// The parsers a worker runs, as progress frames and failure counters
/// name them. A new parser (an archive format, say) is one more variant
/// here and one more step in [`crate::answer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Parser {
    Mime,
    Html2text,
    AttachmentInside,
    BayesTokenizer,
    CleanHtml,
    ReportDecompress,
    Office,
    Pdf,
}

impl Parser {
    pub const ALL: [Parser; 8] = [
        Parser::Mime,
        Parser::Html2text,
        Parser::AttachmentInside,
        Parser::BayesTokenizer,
        Parser::CleanHtml,
        Parser::ReportDecompress,
        Parser::Office,
        Parser::Pdf,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Parser::Mime => "mime",
            Parser::Html2text => "html2text",
            Parser::AttachmentInside => "attachment_inside",
            Parser::BayesTokenizer => "bayes_tokenizer",
            Parser::CleanHtml => "clean_html",
            Parser::ReportDecompress => "report_decompress",
            Parser::Office => "office",
            Parser::Pdf => "pdf",
        }
    }

    fn code(self) -> u8 {
        Parser::ALL.iter().position(|p| *p == self).unwrap_or(0) as u8
    }

    fn from_code(c: u8) -> Option<Parser> {
        Parser::ALL.get(usize::from(c)).copied()
    }
}

/// [`mailrs_fraud::attachment_inside::Inside`] on the wire. A field added
/// to `Inside` is added here and in the two conversions below.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InsideWire {
    pub carries_code: bool,
    pub encrypted_document_archive: bool,
}

impl From<mailrs_fraud::attachment_inside::Inside> for InsideWire {
    fn from(i: mailrs_fraud::attachment_inside::Inside) -> Self {
        Self {
            carries_code: i.carries_code,
            encrypted_document_archive: i.encrypted_document_archive,
        }
    }
}

impl From<InsideWire> for mailrs_fraud::attachment_inside::Inside {
    fn from(i: InsideWire) -> Self {
        Self {
            carries_code: i.carries_code,
            encrypted_document_archive: i.encrypted_document_archive,
        }
    }
}

/// What the first HTML part's markup says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HtmlFacts {
    pub has_tracking_pixel: bool,
    pub is_template_heavy: bool,
    pub link_count: usize,
}

/// The readings of one message; a reading not asked for is empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageReading {
    pub body_text: Option<String>,
    pub inside: InsideWire,
    pub tokens: Vec<String>,
    /// `None` when the message has no HTML part or it was not asked for.
    pub html_facts: Option<HtmlFacts>,
}

/// An answer's header; report payloads travel as blobs after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum AnswerHeader {
    Message(MessageReading),
    ReportPayloads,
    /// Lines of text, or why the file could not be read.
    Office {
        lines: Result<Vec<String>, String>,
    },
    Pdf {
        pages: Result<Vec<String>, String>,
    },
}

/// A full answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub header: AnswerHeader,
    pub blobs: Vec<Vec<u8>>,
}

pub fn write_frame(w: &mut impl Write, kind: u8, parts: &[&[u8]]) -> io::Result<()> {
    let len: usize = 1 + parts.iter().map(|p| p.len()).sum::<usize>();
    let len = u32::try_from(len).map_err(|_| io::Error::other("frame too large"))?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&[kind])?;
    for p in parts {
        w.write_all(p)?;
    }
    w.flush()
}

/// One frame, or `None` at a clean end of stream.
pub fn read_frame(r: &mut impl Read) -> io::Result<Option<(u8, Vec<u8>)>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len == 0 || len > MAX_FRAME {
        return Err(io::Error::other(format!("frame of {len} bytes")));
    }
    let mut kind = [0u8; 1];
    r.read_exact(&mut kind)?;
    let mut body = vec![0u8; len - 1];
    r.read_exact(&mut body)?;
    Ok(Some((kind[0], body)))
}

pub fn write_request(w: &mut impl Write, op: &Op, input: &[u8]) -> io::Result<()> {
    let header = serde_json::to_vec(op).map_err(io::Error::other)?;
    let hlen = (header.len() as u32).to_le_bytes();
    write_frame(w, REQUEST, &[&hlen, &header, input])
}

/// The op and the input bytes of a request body.
pub fn read_request(body: &[u8]) -> io::Result<(Op, &[u8])> {
    let (header, rest) = split_header(body)?;
    let op = serde_json::from_slice(header).map_err(io::Error::other)?;
    Ok((op, rest))
}

pub fn write_answer(w: &mut impl Write, answer: &Answer) -> io::Result<()> {
    let header = serde_json::to_vec(&answer.header).map_err(io::Error::other)?;
    let hlen = (header.len() as u32).to_le_bytes();
    let lens: Vec<[u8; 4]> = answer
        .blobs
        .iter()
        .map(|b| (b.len() as u32).to_le_bytes())
        .collect();
    let mut parts: Vec<&[u8]> = vec![&hlen, &header];
    for (len, blob) in lens.iter().zip(&answer.blobs) {
        parts.push(len);
        parts.push(blob);
    }
    write_frame(w, ANSWER, &parts)
}

pub fn read_answer(body: &[u8]) -> io::Result<Answer> {
    let (header, mut rest) = split_header(body)?;
    let header = serde_json::from_slice(header).map_err(io::Error::other)?;
    let mut blobs = Vec::new();
    while !rest.is_empty() {
        let (blob, more) = split_header(rest)?;
        blobs.push(blob.to_vec());
        rest = more;
    }
    Ok(Answer { header, blobs })
}

pub fn write_progress(w: &mut impl Write, parser: Parser) -> io::Result<()> {
    write_frame(w, PROGRESS, &[&[parser.code()]])
}

pub fn read_progress(body: &[u8]) -> Option<Parser> {
    body.first().and_then(|c| Parser::from_code(*c))
}

fn split_header(body: &[u8]) -> io::Result<(&[u8], &[u8])> {
    let len = body
        .get(..4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
        .ok_or_else(|| io::Error::other("short frame"))?;
    let end = 4usize
        .checked_add(len)
        .filter(|e| *e <= body.len())
        .ok_or_else(|| io::Error::other("length past the frame"))?;
    Ok((&body[4..end], &body[end..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_and_an_answer_survive_the_pipe() {
        let mut buf = Vec::new();
        let op = Op::Message(Want {
            body_text: true,
            tokens: Some(Tokenizer::Raw),
            ..Want::default()
        });
        write_request(&mut buf, &op, b"raw bytes").unwrap();
        let (kind, body) = read_frame(&mut &buf[..]).unwrap().unwrap();
        assert_eq!(kind, REQUEST);
        let (back, input) = read_request(&body).unwrap();
        assert_eq!(back, op);
        assert_eq!(input, b"raw bytes");

        let answer = Answer {
            header: AnswerHeader::ReportPayloads,
            blobs: vec![b"<xml/>".to_vec(), vec![0, 159, 255], Vec::new()],
        };
        let mut buf = Vec::new();
        write_answer(&mut buf, &answer).unwrap();
        let (kind, body) = read_frame(&mut &buf[..]).unwrap().unwrap();
        assert_eq!(kind, ANSWER);
        assert_eq!(read_answer(&body).unwrap(), answer);
    }

    #[test]
    fn every_parser_names_itself_in_a_progress_frame() {
        for p in Parser::ALL {
            let mut buf = Vec::new();
            write_progress(&mut buf, p).unwrap();
            let (kind, body) = read_frame(&mut &buf[..]).unwrap().unwrap();
            assert_eq!(kind, PROGRESS);
            assert_eq!(read_progress(&body), Some(p));
        }
    }

    #[test]
    fn a_frame_claiming_more_than_it_carries_is_an_error_not_a_hang() {
        let mut buf = Vec::new();
        write_frame(&mut buf, ANSWER, &[&[9, 0, 0, 0], b"{}"]).unwrap();
        let (_, body) = read_frame(&mut &buf[..]).unwrap().unwrap();
        assert!(read_answer(&body).is_err());
        assert!(read_frame(&mut &[0u8, 0, 0, 0][..]).is_err());
        assert!(read_frame(&mut &[][..]).unwrap().is_none());
    }
}
