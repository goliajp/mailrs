//! Resident workers: started on first use, one request at a time each,
//! killed past their wall-clock limit, started again when they die, with
//! a growing pause when they keep dying.

use std::io::BufReader;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::health::{self, Kind};
use crate::proto::{self, Answer, Op, Parser};

/// The longest pause before a worker that keeps dying is started again.
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// A resident worker holding more than this after an answer is retired
/// and started afresh on the next request: the allocator keeps what the
/// largest message needed (464 MB after a pass over the production corpus,
/// 158 MB for the largest message alone), well under the worker's own
/// address-space limit of 768 MiB.
const RETIRE_RSS: u64 = 384 * 1024 * 1024;

/// How a request ended without an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The worker died, was killed or answered nonsense while it had this
    /// input: the input is the likely cause.
    OnInput { parser: Parser, kind: Kind },
    /// No worker could take the request: starting one failed, or it is
    /// pausing after dying several times in a row. Says nothing about
    /// this input.
    Unavailable,
}

/// Where the worker binary is and how many to keep.
#[derive(Debug, Clone)]
pub struct Setup {
    pub bin: PathBuf,
    /// Workers kept at once; requests beyond that wait for one.
    pub workers: usize,
    /// Extra environment for the worker (tests turn on fault injection).
    pub env: Vec<(String, String)>,
}

impl Setup {
    /// `MAILRS_PARSE_WORKER_BIN`, or `mailrs-parse-worker` next to this
    /// binary.
    pub fn from_env(workers: usize) -> Result<Self, String> {
        let bin = match std::env::var_os("MAILRS_PARSE_WORKER_BIN") {
            Some(b) => PathBuf::from(b),
            None => std::env::current_exe()
                .map_err(|e| format!("locating this binary: {e}"))?
                .with_file_name("mailrs-parse-worker"),
        };
        if !bin.is_file() {
            return Err(format!("{} not found", bin.display()));
        }
        Ok(Self {
            bin,
            workers,
            env: Vec::new(),
        })
    }
}

enum Frame {
    Progress(Parser),
    Answer(Answer),
    Error(String),
    /// The pipe closed, or carried something that is not a frame.
    End,
}

struct Live {
    child: Child,
    stdin: ChildStdin,
    frames: mpsc::Receiver<Frame>,
}

#[derive(Default)]
struct Slot {
    live: Option<Live>,
    started_once: bool,
    deaths_in_a_row: u32,
    retry_at: Option<Instant>,
}

pub struct Pool {
    setup: Setup,
    slots: Vec<Mutex<Slot>>,
    /// Each slot's current worker process id, 0 for none; readable while
    /// the slot is busy.
    pids: Vec<AtomicU32>,
    next: AtomicUsize,
}

impl Pool {
    pub fn new(setup: Setup) -> Self {
        let n = setup.workers.max(1);
        Self {
            slots: (0..n).map(|_| Mutex::new(Slot::default())).collect(),
            pids: (0..n).map(|_| AtomicU32::new(0)).collect(),
            setup,
            next: AtomicUsize::new(0),
        }
    }

    /// Ask a worker. Blocking; waits for a free worker first, and the
    /// limit counts from when the request is sent.
    pub fn call(&self, op: &Op, input: &[u8], limit: Duration) -> Result<Answer, Failure> {
        let start = self.next.fetch_add(1, Ordering::Relaxed);
        let n = self.slots.len();
        let free = (0..n).find_map(|i| {
            let at = (start + i) % n;
            self.slots[at].try_lock().ok().map(|s| (at, s))
        });
        let (at, mut slot) = match free {
            Some(s) => s,
            None => (
                start % n,
                self.slots[start % n]
                    .lock()
                    .map_err(|_| Failure::Unavailable)?,
            ),
        };
        call_in(&self.setup, &mut slot, &self.pids[at], op, input, limit)
    }

    /// The process ids of the running workers.
    pub fn worker_pids(&self) -> Vec<u32> {
        self.pids
            .iter()
            .map(|p| p.load(Ordering::Relaxed))
            .filter(|p| *p != 0)
            .collect()
    }

    /// One request in a worker of its own, which exits once it has
    /// answered: for work that is rare and long, kept apart from the
    /// resident workers.
    pub fn call_once(&self, op: &Op, input: &[u8], limit: Duration) -> Result<Answer, Failure> {
        let mut slot = Slot::default();
        let pid = AtomicU32::new(0);
        let got = call_in(&self.setup, &mut slot, &pid, op, input, limit);
        if let Some(mut live) = slot.live.take() {
            drop(live.stdin);
            let _ = live.child.wait();
        }
        got
    }
}

fn call_in(
    setup: &Setup,
    slot: &mut Slot,
    pid: &AtomicU32,
    op: &Op,
    input: &[u8],
    limit: Duration,
) -> Result<Answer, Failure> {
    // a worker that died since its last answer died of something else
    if let Some(live) = slot.live.as_mut()
        && !matches!(live.child.try_wait(), Ok(None))
    {
        tracing::warn!(
            event = "parse_worker_died_idle",
            "a parse worker died between requests"
        );
        reap(slot);
        pid.store(0, Ordering::Relaxed);
        died(slot);
    }
    if slot.live.is_none() {
        if slot.retry_at.is_some_and(|t| Instant::now() < t) {
            return Err(Failure::Unavailable);
        }
        match spawn(setup) {
            Ok(live) => {
                if slot.started_once {
                    health::record_restart();
                }
                slot.started_once = true;
                pid.store(live.child.id(), Ordering::Relaxed);
                slot.live = Some(live);
            }
            Err(e) => {
                tracing::error!(event = "parse_worker_start_failed", error = %e);
                died(slot);
                return Err(Failure::Unavailable);
            }
        }
    }
    let Some(live) = slot.live.as_mut() else {
        return Err(Failure::Unavailable);
    };
    let got = exchange(live, op, input, limit);
    match got {
        Ok(answer) => {
            slot.deaths_in_a_row = 0;
            slot.retry_at = None;
            if resident_bytes(live.child.id()).is_some_and(|b| b > RETIRE_RSS) {
                retire(slot);
                pid.store(0, Ordering::Relaxed);
            }
            Ok(answer)
        }
        Err((parser, kind)) => {
            health::record_failure(parser, kind);
            tracing::warn!(
                event = "parse_worker_failed",
                parser = parser.as_str(),
                kind = kind.as_str(),
                "a parse worker failed on this input"
            );
            reap(slot);
            pid.store(0, Ordering::Relaxed);
            died(slot);
            Err(Failure::OnInput { parser, kind })
        }
    }
}

/// Send the request and read until the answer. `Err` names the parser
/// the worker last said it was in and how it ended; the worker is then
/// dead or about to be killed by [`reap`].
fn exchange(
    live: &mut Live,
    op: &Op,
    input: &[u8],
    limit: Duration,
) -> Result<Answer, (Parser, Kind)> {
    let deadline = Instant::now() + limit;
    let mut parser = Parser::Mime;
    if proto::write_request(&mut live.stdin, op, input).is_err() {
        return Err((parser, ended(&mut live.child)));
    }
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match live.frames.recv_timeout(left) {
            Ok(Frame::Progress(p)) => parser = p,
            Ok(Frame::Answer(a)) => return Ok(a),
            Ok(Frame::Error(e)) => {
                tracing::warn!(event = "parse_worker_error", error = %e);
                return Err((parser, Kind::Error));
            }
            Ok(Frame::End) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err((parser, ended(&mut live.child)));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = live.child.kill();
                let _ = live.child.wait();
                return Err((parser, Kind::Timeout));
            }
        }
    }
}

/// How a worker whose pipe closed ended. The kill only matters for one
/// that closed its pipe and kept running; one already exiting keeps the
/// status it exited with.
fn ended(child: &mut Child) -> Kind {
    let _ = child.kill();
    match child.wait() {
        Ok(status) => kind_of(status),
        Err(_) => Kind::Killed,
    }
}

#[cfg(unix)]
fn kind_of(status: ExitStatus) -> Kind {
    use std::os::unix::process::ExitStatusExt;
    // SIGABRT is what a panic comes to under panic = "abort"; 101 is a
    // panic that unwound out of main
    match (status.signal(), status.code()) {
        (Some(6), _) | (None, Some(101)) => Kind::Panic,
        (Some(_), _) => Kind::Killed,
        _ => Kind::Error,
    }
}

#[cfg(not(unix))]
fn kind_of(_status: ExitStatus) -> Kind {
    Kind::Killed
}

/// Close a worker's input and wait for it to exit, which it does at the
/// end of its input.
fn retire(slot: &mut Slot) {
    if let Some(mut live) = slot.live.take() {
        drop(live.stdin);
        let _ = live.child.wait();
        tracing::info!(
            event = "parse_worker_retired",
            "a parse worker grew large and was retired"
        );
    }
}

/// Resident memory of `pid`, from `/proc`; `None` where there is none.
fn resident_bytes(pid: u32) -> Option<u64> {
    let statm = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * rustix::param::page_size() as u64)
}

fn reap(slot: &mut Slot) {
    if let Some(mut live) = slot.live.take() {
        let _ = live.child.kill();
        let _ = live.child.wait();
    }
}

/// The first death is started again on the next request; each one after
/// it without an answer in between doubles a pause, up to a minute.
fn died(slot: &mut Slot) {
    slot.deaths_in_a_row += 1;
    slot.retry_at = match slot.deaths_in_a_row {
        0 | 1 => None,
        n => Some(Instant::now() + backoff(n)),
    };
}

fn backoff(deaths_in_a_row: u32) -> Duration {
    let secs = 1u64 << deaths_in_a_row.saturating_sub(2).min(6);
    Duration::from_secs(secs).min(MAX_BACKOFF)
}

fn spawn(setup: &Setup) -> std::io::Result<Live> {
    let mut child = Command::new(&setup.bin)
        .envs(setup.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(std::io::Error::other("child pipes missing"));
    };
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("parse-worker-read".into())
        .spawn(move || {
            let mut r = BufReader::new(stdout);
            loop {
                let frame = match proto::read_frame(&mut r) {
                    Ok(Some((proto::PROGRESS, body))) => match proto::read_progress(&body) {
                        Some(p) => Frame::Progress(p),
                        None => Frame::End,
                    },
                    Ok(Some((proto::ANSWER, body))) => match proto::read_answer(&body) {
                        Ok(a) => Frame::Answer(a),
                        Err(e) => Frame::Error(format!("unreadable answer: {e}")),
                    },
                    Ok(Some((proto::ERROR, body))) => {
                        Frame::Error(String::from_utf8_lossy(&body).into_owned())
                    }
                    Ok(Some(_)) | Ok(None) | Err(_) => Frame::End,
                };
                let end = matches!(frame, Frame::End);
                if tx.send(frame).is_err() || end {
                    return;
                }
            }
        })?;
    Ok(Live {
        child,
        stdin,
        frames: rx,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_worker_that_keeps_dying_waits_longer_each_time_up_to_a_minute() {
        let mut slot = Slot::default();
        died(&mut slot);
        assert!(slot.retry_at.is_none(), "the first death restarts at once");
        let waits: Vec<u64> = (2..12).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(waits, [1, 2, 4, 8, 16, 32, 60, 60, 60, 60]);
    }
}
