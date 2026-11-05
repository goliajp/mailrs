//! Parsing untrusted mail bytes in a process that can die in our place.
//!
//! html2text, the Office and PDF readers, report decompression, the
//! attachment listing and the Bayes tokenizers all read bytes a stranger
//! chose. A panic in them aborts the process (release builds are
//! `panic = "abort"`), a loop holds a thread forever, an allocation that
//! balloons takes the container's memory. Here they run in
//! `mailrs-parse-worker`, a resident child that sets its own memory, CPU
//! and priority limits; the caller adds a wall-clock limit per request
//! and kills a worker past it.
//!
//! A process calls [`install`] once at start. Until it does (tests,
//! tools) the same parsers run in process, through the same functions.
//!
//! Every call says how it went:
//! - [`Outcome::Read`]: the answer.
//! - [`Outcome::FailedOnInput`]: the worker died, was killed or answered
//!   nonsense while it held this input. The input is what to suspect;
//!   callers record it as a fact about the message and carry on without
//!   the reading.
//! - [`Outcome::Unavailable`]: no worker could take it (starting failed,
//!   or one that keeps dying is pausing). Says nothing about the input;
//!   callers carry on without the reading and record nothing about it.
//!
//! Neither failure is a reason to refuse, hold or set aside a message.

pub mod answer;
pub mod health;
mod pool;
pub mod proto;

use std::sync::OnceLock;
use std::time::Duration;

pub use health::Kind;
pub use pool::{Failure, Pool, Setup};
pub use proto::{HtmlFacts, MessageReading, OfficeKind, Parser, Tokenizer, Want};

use proto::{Answer, AnswerHeader, Op};

/// The wall-clock limit of a request, by who is waiting for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    /// The receiver during SMTP DATA, and the spool drain.
    Inbound,
    /// Re-judging, backfills, sweeps and the offline checker: nobody is
    /// on the line, and a large message may take its time.
    Batch,
    /// One attachment's text in a worker of its own.
    Document,
}

impl Limit {
    pub fn duration(self) -> Duration {
        match self {
            Limit::Inbound => INBOUND_LIMIT,
            Limit::Batch => BATCH_LIMIT,
            Limit::Document => DOCUMENT_LIMIT,
        }
    }
}

/// See the parse-time measurement in the containment notes: the slowest
/// legitimate message of the production corpus stays well inside it.
pub const INBOUND_LIMIT: Duration = Duration::from_secs(2);
pub const BATCH_LIMIT: Duration = Duration::from_secs(10);
pub const DOCUMENT_LIMIT: Duration = Duration::from_secs(120);

/// How one call went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome<T> {
    Read(T),
    FailedOnInput { parser: Parser, kind: Kind },
    Unavailable,
}

impl<T> Outcome<T> {
    /// The reading, or `None` after either failure.
    pub fn read(self) -> Option<T> {
        match self {
            Outcome::Read(t) => Some(t),
            _ => None,
        }
    }

    /// Whether a worker failed on this input.
    pub fn failed_on_input(&self) -> bool {
        matches!(self, Outcome::FailedOnInput { .. })
    }

    fn map<U>(self, f: impl FnOnce(T) -> U) -> Outcome<U> {
        match self {
            Outcome::Read(t) => Outcome::Read(f(t)),
            Outcome::FailedOnInput { parser, kind } => Outcome::FailedOnInput { parser, kind },
            Outcome::Unavailable => Outcome::Unavailable,
        }
    }
}

static POOL: OnceLock<pool::Pool> = OnceLock::new();

/// Run parsing in workers from now on, for the process named `process`
/// in the failure counters. A second call changes nothing.
pub fn install(process: &'static str, setup: Setup) {
    if POOL.set(pool::Pool::new(setup)).is_ok() {
        health::set_installed(process);
    }
}

fn call(op: Op, input: &[u8], limit: Limit, once: bool) -> Outcome<Answer> {
    let Some(pool) = POOL.get() else {
        return Outcome::Read(answer::answer(&op, input, &mut |_| {}));
    };
    let got = match once {
        true => pool.call_once(&op, input, limit.duration()),
        false => pool.call(&op, input, limit.duration()),
    };
    match got {
        Ok(a) => Outcome::Read(a),
        Err(pool::Failure::OnInput { parser, kind }) => Outcome::FailedOnInput { parser, kind },
        Err(pool::Failure::Unavailable) => Outcome::Unavailable,
    }
}

/// An answer of another shape than the request asked for: the worker is
/// not one this process speaks to.
fn mismatched<T>(op: &str) -> Outcome<T> {
    tracing::error!(
        event = "parse_worker_mismatch",
        op,
        "the worker answered another request"
    );
    Outcome::FailedOnInput {
        parser: Parser::Mime,
        kind: Kind::Error,
    }
}

/// The readings of one whole message.
pub fn read_message(raw: &[u8], want: Want, limit: Limit) -> Outcome<MessageReading> {
    match call(Op::Message(want), raw, limit, false) {
        Outcome::Read(Answer {
            header: AnswerHeader::Message(r),
            ..
        }) => Outcome::Read(r),
        Outcome::Read(_) => mismatched("message"),
        other => other.map(|_| MessageReading::default()),
    }
}

/// [`mailrs_clean::body_text_for_search`] of a raw message.
pub fn body_text(raw: &[u8], limit: Limit) -> Outcome<Option<String>> {
    let want = Want {
        body_text: true,
        ..Want::default()
    };
    read_message(raw, want, limit).map(|r| r.body_text)
}

/// Bayes tokens of a raw message.
pub fn tokens(raw: &[u8], tokenizer: Tokenizer, limit: Limit) -> Outcome<Vec<String>> {
    let want = Want {
        tokens: Some(tokenizer),
        ..Want::default()
    };
    read_message(raw, want, limit).map(|r| r.tokens)
}

/// The decompressed report attachments of a raw message
/// ([`mailrs_dmarc::ingest::extract_report_payloads`]).
pub fn report_payloads(raw: &[u8], limit: Limit) -> Outcome<Vec<Vec<u8>>> {
    match call(Op::ReportPayloads, raw, limit, false) {
        Outcome::Read(Answer {
            header: AnswerHeader::ReportPayloads,
            blobs,
        }) => Outcome::Read(blobs),
        Outcome::Read(_) => mismatched("report_payloads"),
        other => other.map(|_| Vec::new()),
    }
}

/// The text lines of one Office file, in a worker of its own.
pub fn office_text(kind: OfficeKind, bytes: &[u8]) -> Outcome<Result<Vec<String>, String>> {
    match call(Op::Office { kind }, bytes, Limit::Document, true) {
        Outcome::Read(Answer {
            header: AnswerHeader::Office { lines },
            ..
        }) => Outcome::Read(lines),
        Outcome::Read(_) => mismatched("office"),
        other => other.map(|_| Ok(Vec::new())),
    }
}

/// The embedded text of one PDF, page by page, in a worker of its own.
pub fn pdf_pages(bytes: &[u8]) -> Outcome<Result<Vec<String>, String>> {
    match call(Op::Pdf, bytes, Limit::Document, true) {
        Outcome::Read(Answer {
            header: AnswerHeader::Pdf { pages },
            ..
        }) => Outcome::Read(pages),
        Outcome::Read(_) => mismatched("pdf"),
        other => other.map(|_| Ok(Vec::new())),
    }
}
