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

enum Backend {
    Workers(pool::Pool),
    /// In process, failing the way a worker would on inputs carrying the
    /// fault header: for tests of what callers do with each outcome.
    #[cfg(feature = "testing")]
    Faults,
}

static BACKEND: OnceLock<Backend> = OnceLock::new();

/// Run parsing in workers from now on, for the process named `process`
/// in the failure counters. A second call changes nothing.
pub fn install(process: &'static str, setup: Setup) {
    if BACKEND
        .set(Backend::Workers(pool::Pool::new(setup)))
        .is_ok()
    {
        health::set_installed(process);
    }
}

/// [`install`] with `workers` resident workers from
/// [`Setup::from_env`], for a server: without the worker binary there is
/// nothing to contain the parsers in and the image is broken, so the
/// process says so and exits.
pub fn install_or_exit(process: &'static str, workers: usize) {
    match Setup::from_env(workers) {
        Ok(setup) => install(process, setup),
        Err(e) => {
            eprintln!("{process}: parse worker: {e}");
            std::process::exit(1);
        }
    }
}

/// Parse in process, except that an input carrying
/// `X-Parse-Worker-Fault: abort` comes back as a panic on that input and
/// one carrying `X-Parse-Worker-Fault: unavailable` as no worker
/// available, the first counted as the worker's would be. For tests of
/// the callers; the real worker's own faults are tested against the
/// binary.
#[cfg(feature = "testing")]
pub fn install_faults(process: &'static str) {
    if BACKEND.set(Backend::Faults).is_ok() {
        health::set_installed(process);
    }
}

#[cfg(feature = "testing")]
fn fault_of(input: &[u8]) -> Option<&'static str> {
    const H: &[u8] = b"X-Parse-Worker-Fault: ";
    let head = &input[..input.len().min(16 * 1024)];
    let at = head.windows(H.len()).position(|w| w == H)?;
    let what = &head[at + H.len()..];
    match (what.starts_with(b"abort"), what.starts_with(b"unavailable")) {
        (true, _) => Some("abort"),
        (_, true) => Some("unavailable"),
        _ => None,
    }
}

fn call(op: Op, input: &[u8], limit: Limit, once: bool) -> Outcome<Answer> {
    let got = match BACKEND.get() {
        None => return Outcome::Read(answer::answer(&op, input, &mut |_| {})),
        Some(Backend::Workers(pool)) => match once {
            true => pool.call_once(&op, input, limit.duration()),
            false => pool.call(&op, input, limit.duration()),
        },
        #[cfg(feature = "testing")]
        Some(Backend::Faults) => match fault_of(input) {
            Some("abort") => {
                health::record_failure(Parser::Html2text, Kind::Panic);
                Err(pool::Failure::OnInput {
                    parser: Parser::Html2text,
                    kind: Kind::Panic,
                })
            }
            Some(_) => Err(pool::Failure::Unavailable),
            None => Ok(answer::answer(&op, input, &mut |_| {})),
        },
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

/// What a message's attachments hold, as the fraud rules read it, and
/// whether reading them failed on this message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AttachmentsRead {
    pub inside: mailrs_fraud::attachment_inside::Inside,
    /// The worker failed on this message; `inside` is then empty.
    pub parser_failed: bool,
}

impl AttachmentsRead {
    /// From a reading that asked for `inside`. An unavailable worker
    /// leaves both empty: that says nothing about the message.
    pub fn of(outcome: &Outcome<MessageReading>) -> Self {
        match outcome {
            Outcome::Read(r) => Self {
                inside: r.inside.into(),
                parser_failed: false,
            },
            Outcome::FailedOnInput { .. } => Self {
                inside: Default::default(),
                parser_failed: true,
            },
            Outcome::Unavailable => Self::default(),
        }
    }
}

/// What the attachments of `raw` hold, each read for what it holds as
/// well as what it is called ([`mailrs_fraud::attachment_inside::read`]).
pub fn attachments_inside(raw: &[u8], limit: Limit) -> AttachmentsRead {
    let want = Want {
        inside: true,
        ..Want::default()
    };
    AttachmentsRead::of(&read_message(raw, want, limit))
}
