//! `mailrs-parse-worker`: answers parse requests on stdin, one at a time,
//! until stdin closes.
//!
//! The parent's wall-clock limit kills a request that loops; the limits
//! set here bound what one request can take before that: address space,
//! CPU seconds per request, and a lower priority than the server.
//!
//! `MAILRS_PARSE_WORKER_FAULTS=1` makes an input carrying
//! `X-Parse-Worker-Fault: abort|hang|exit|garbage` in its first 16 KiB do
//! that instead of being parsed. Tests and the release smoke use it to
//! fail the worker on one message; nothing sets it in production.

use std::io::{BufReader, BufWriter, Write};

use mailrs_parse_worker::{answer, proto};
use rustix::process::{Resource, Rlimit, getrlimit, setpriority_process, setrlimit};

/// Address space. Over a copy of the production maildir (45,575
/// messages) one worker reading every message peaked at 464 MB resident,
/// and a fresh worker on the largest single message (23.8 MB) at 158 MB;
/// address space runs higher than resident memory.
#[cfg(target_os = "linux")]
///
/// Sized so that the two resident workers and the one-off document worker
/// fit beside fastcore and the OCR child in fastcore's container (6g):
/// 2.5 GB fastcore + 2 × 768 MiB + 1.5 GiB OCR (the document worker and
/// OCR never run at once) leaves about 400 MB. The client retires a
/// resident worker past 384 MiB resident, so growth from the allocator
/// keeping freed memory never meets this limit on an ordinary message.
const MEMORY_LIMIT: u64 = 768 * 1024 * 1024;
/// CPU seconds one request may use, past what the worker had used
/// before it. Above the longest wall-clock limit a caller sets.
const CPU_PER_REQUEST: u64 = 150;
/// Below the server, not at the bottom: the receiver waits on this
/// process during DATA, and at 19 a busy host (the OCR child runs at 19
/// beside normal-priority work) would stretch an ordinary parse towards
/// the wall-clock limit and blame the message for it.
const NICE: i32 = 10;
const FAULT_HEADER: &[u8] = b"X-Parse-Worker-Fault: ";

fn main() {
    if let Err(e) = limit_memory().and_then(|()| setpriority_process(None, NICE)) {
        fail(&format!("setting limits: {e}"));
    }
    let faults = std::env::var_os("MAILRS_PARSE_WORKER_FAULTS").is_some_and(|v| v == "1");
    let mut input = BufReader::new(std::io::stdin().lock());
    let mut out = BufWriter::new(std::io::stdout().lock());
    loop {
        let (kind, body) = match proto::read_frame(&mut input) {
            Ok(Some(frame)) => frame,
            Ok(None) => return,
            Err(e) => fail(&format!("reading a request: {e}")),
        };
        if kind != proto::REQUEST {
            fail(&format!("frame of kind {kind} where a request belongs"));
        }
        if let Err(e) = limit_cpu() {
            fail(&format!("setting the cpu limit: {e}"));
        }
        let written = match proto::read_request(&body) {
            Ok((op, bytes)) => {
                if faults {
                    fault(&mut out, bytes);
                }
                let answer = answer::answer(&op, bytes, &mut |p| {
                    if let Err(e) = proto::write_progress(&mut out, p) {
                        fail(&format!("writing progress: {e}"));
                    }
                });
                proto::write_answer(&mut out, &answer)
            }
            Err(e) => proto::write_frame(&mut out, proto::ERROR, &[e.to_string().as_bytes()]),
        };
        if let Err(e) = written {
            fail(&format!("writing the answer: {e}"));
        }
    }
}

fn fault(out: &mut impl Write, bytes: &[u8]) {
    let head = &bytes[..bytes.len().min(16 * 1024)];
    let Some(at) = head
        .windows(FAULT_HEADER.len())
        .position(|w| w == FAULT_HEADER)
    else {
        return;
    };
    let what = &head[at + FAULT_HEADER.len()..];
    let _ = proto::write_progress(out, proto::Parser::Html2text);
    if what.starts_with(b"abort") {
        std::process::abort();
    } else if what.starts_with(b"hang") {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    } else if what.starts_with(b"exit") {
        std::process::exit(3);
    } else if what.starts_with(b"garbage") {
        let _ = proto::write_frame(out, proto::ANSWER, &[b"not an answer"]);
    }
}

/// The soft CPU limit, raised before each request to what was used so
/// far plus [`CPU_PER_REQUEST`], so a worker that lives for weeks is not
/// stopped by the sum of its ordinary requests.
fn limit_cpu() -> rustix::io::Result<()> {
    let used = rustix::time::clock_gettime(rustix::time::ClockId::ProcessCPUTime).tv_sec as u64 + 1;
    let hard = getrlimit(Resource::Cpu).maximum;
    let soft = hard.map_or(used + CPU_PER_REQUEST, |h| (used + CPU_PER_REQUEST).min(h));
    setrlimit(
        Resource::Cpu,
        Rlimit {
            current: Some(soft),
            maximum: hard,
        },
    )
}

/// Production runs on Linux. macOS refuses to lower the address-space
/// limit (EINVAL), so a development machine runs without it.
#[cfg(target_os = "linux")]
fn limit_memory() -> rustix::io::Result<()> {
    setrlimit(
        Resource::As,
        Rlimit {
            current: Some(MEMORY_LIMIT),
            maximum: Some(MEMORY_LIMIT),
        },
    )
}

#[cfg(not(target_os = "linux"))]
fn limit_memory() -> rustix::io::Result<()> {
    Ok(())
}

fn fail(msg: &str) -> ! {
    eprintln!("mailrs-parse-worker: {msg}");
    std::process::exit(1);
}
