//! Sustained SMTP-receive load bench.
//!
//! Purpose: substantiate (or refute) the "+10-20% throughput" claim from
//! commit `9f21e0b` ("perf-first release profile"). PERFORMANCE.md tags
//! that claim as unmeasured; this bench produces reproducible numbers
//! so it can move into the "Measured" table — or be retracted entirely.
//!
//! What this benches
//! -----------------
//! The bench drives the same minimal in-process SMTP handler used by
//! `crates/server/tests/e2e.rs` (parse → session state machine → Maildir
//! deliver). That is the slice of mailrs-server that the perf-first
//! profile actually changes — `lto = "fat"` + `codegen-units = 1` +
//! `panic = "abort"` affect cross-crate inlining and unwind tables, both
//! of which apply equally to this in-process binary and the real
//! `mailrs-server` binary, because both link the same `mailrs-smtp-proto`
//! / `mailrs-maildir` / tokio.
//!
//! What this does NOT bench
//! ------------------------
//! The real `mailrs-server` inbound pipeline (SPF/DKIM/DMARC/sieve/PG/
//! Valkey writes). Those need a full integration environment (Postgres,
//! Valkey, DNS) and produce variance much larger than the LTO delta we
//! are trying to detect. Treat the numbers from this bench as a *lower
//! bound* on the LTO impact — the real server has more cross-crate
//! inline opportunities in its hot path.
//!
//! Running
//! -------
//! Both profiles need their own build of this bench. The workspace
//! `Cargo.toml` declares two profiles for the comparison:
//!
//!   - `release`     — perf-first (lto=fat, cgu=1, panic=abort)
//!   - `release-vanilla` — defaults restored (lto=false, cgu=16, panic=unwind)
//!
//! ```bash
//! # perf-first profile (current release default)
//! cargo build --release -p mailrs-server --bench smtp_load
//! "$CARGO_TARGET_DIR/release/deps/smtp_load-*" --duration 30 --conns 32
//!
//! # vanilla release profile
//! cargo build --profile release-vanilla -p mailrs-server --bench smtp_load
//! "$CARGO_TARGET_DIR/release-vanilla/deps/smtp_load-*" --duration 30 --conns 32
//! ```
//!
//! Or use the wrapper script `scripts/bench-smtp-load.sh` which builds
//! both, runs each 3 times, and prints a comparison table.
//!
//! Methodology
//! -----------
//! - Spawn the in-process SMTP server on a random localhost port.
//! - Open N concurrent TCP clients (default 32).
//! - Each client loops EHLO → MAIL FROM → RCPT TO → DATA → body → `.` →
//!   close, opening a fresh TCP connection per message. Per-message
//!   wall-clock latency is recorded.
//! - Run for D seconds (default 30). Report msg/sec sustained plus
//!   median / p99 / p999 latency.
//!
//! `--no-deliver` mode (recommended for LTO comparison)
//! ----------------------------------------------------
//! By default the bench writes one Maildir file per delivered message,
//! which calls `file.sync_all()` (fsync) on every message. Under
//! concurrent load this disk-fsync queue dominates wall-clock latency
//! and *masks* the LTO/CGU/panic CPU-side delta we want to measure —
//! variance from page-cache / APFS behaviour easily hits ±30% between
//! rounds.
//!
//! `--no-deliver` skips the Maildir write but keeps everything else
//! (TCP, codec, `parse_command`, `Session` state machine, response
//! formatting, `unstuff_data`). That's the slice of work the perf-first
//! profile actually changes. Use this mode for the perf-first vs vanilla
//! comparison.

use std::env;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::{Buf, BytesMut};
use futures_util::{SinkExt, StreamExt};
use mailrs_maildir::Maildir;
use mailrs_smtp_proto::response::{format_ehlo_response, Response};
use mailrs_smtp_proto::session::{Event, Session, SessionConfig};
use mailrs_smtp_proto::{parse_command, unstuff_data};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio_util::codec::{Decoder, Encoder, Framed};

// ----- in-process SMTP server (mirrors handle_test_connection from e2e.rs) -----

struct Codec {
    data_mode: bool,
}

impl Codec {
    fn new() -> Self {
        Self { data_mode: false }
    }
}

#[derive(Debug)]
enum Input {
    Command(String),
    Data(Vec<u8>),
}

impl Decoder for Codec {
    type Item = Input;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if self.data_mode {
            if let Some(pos) = src
                .windows(5)
                .position(|w| w == b"\r\n.\r\n")
                .map(|p| p + 2)
            {
                let data = src.split_to(pos + 3).to_vec();
                self.data_mode = false;
                return Ok(Some(Input::Data(data)));
            }
            Ok(None)
        } else if let Some(pos) = src.windows(2).position(|w| w == b"\r\n") {
            let line = src.split_to(pos);
            src.advance(2);
            Ok(Some(Input::Command(
                String::from_utf8_lossy(&line).into_owned(),
            )))
        } else {
            Ok(None)
        }
    }
}

impl Encoder<String> for Codec {
    type Error = std::io::Error;

    fn encode(&mut self, item: String, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.extend_from_slice(item.as_bytes());
        Ok(())
    }
}

async fn handle_connection(stream: TcpStream, maildir_root: Arc<String>, no_deliver: bool) {
    let hostname = "mx.bench.local";
    let config = SessionConfig::default();
    let mut session = Session::new(hostname, config);
    let mut framed = Framed::new(stream, Codec::new());

    if framed
        .send(Response::greeting(hostname).format_greeting())
        .await
        .is_err()
    {
        return;
    }

    while let Some(Ok(input)) = framed.next().await {
        match input {
            Input::Command(line) => match parse_command(&line) {
                Ok(cmd) => {
                    if matches!(
                        cmd,
                        mailrs_smtp_proto::Command::Ehlo(_) | mailrs_smtp_proto::Command::Helo(_)
                    ) {
                        let event = session.handle_command(&cmd);
                        if matches!(event, Event::Reply(ref r) if r.code == 250) {
                            let caps = session.capabilities();
                            let resp = format_ehlo_response(hostname, &caps);
                            if framed.send(resp).await.is_err() {
                                return;
                            }
                            continue;
                        }
                    }

                    let event = session.handle_command(&cmd);
                    match event {
                        Event::Reply(resp) => {
                            if framed.send(resp.format()).await.is_err() {
                                return;
                            }
                        }
                        Event::NeedData { forward_paths, .. } => {
                            if framed
                                .send(Response::data_start().format())
                                .await
                                .is_err()
                            {
                                return;
                            }
                            framed.codec_mut().data_mode = true;

                            if let Some(Ok(Input::Data(raw))) = framed.next().await {
                                let body = unstuff_data(&raw);
                                let mut ok = true;
                                if no_deliver {
                                    // CPU-only mode: still unstuff + iterate, but
                                    // skip the fsync'd Maildir write. The disk
                                    // path dominates wall-clock under load and
                                    // masks the LTO/CGU/panic delta we want to
                                    // measure.
                                    std::hint::black_box(&body);
                                } else {
                                    for rcpt in &forward_paths {
                                        if let Some((local, domain)) = rcpt.split_once('@') {
                                            let path = format!(
                                                "{}/{domain}/{local}",
                                                maildir_root.as_str()
                                            );
                                            match Maildir::create(&path) {
                                                Ok(md) => {
                                                    if md.deliver(&body).is_err() {
                                                        ok = false;
                                                    }
                                                }
                                                Err(_) => ok = false,
                                            }
                                        }
                                    }
                                }
                                let resp = if ok {
                                    Response::data_ok()
                                } else {
                                    Response::new(451, None, "error")
                                };
                                if framed.send(resp.format()).await.is_err() {
                                    return;
                                }
                            } else {
                                return;
                            }
                        }
                        Event::Shutdown(resp) => {
                            framed.send(resp.format()).await.ok();
                            return;
                        }
                        _ => {
                            if framed.send(Response::bad_sequence().format()).await.is_err() {
                                return;
                            }
                        }
                    }
                }
                Err(_) => {
                    if framed.send(Response::syntax_error().format()).await.is_err() {
                        return;
                    }
                }
            },
            Input::Data(_) => return,
        }
    }
}

async fn start_server(maildir_root: Arc<String>, no_deliver: bool) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let root = maildir_root.clone();
                    tokio::spawn(async move { handle_connection(stream, root, no_deliver).await });
                }
                Err(_) => return,
            }
        }
    });
    port
}

// ----- client driver -----

/// One SMTP transaction over a fresh TCP connection. Returns the
/// per-message wall-clock latency, or `None` if the session errored.
async fn one_session(port: u16, body: &[u8]) -> Option<Duration> {
    let start = Instant::now();
    let stream = TcpStream::connect(("127.0.0.1", port)).await.ok()?;
    stream.set_nodelay(true).ok();
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);

    // helper: read either single-line or multi-line response, return final
    // status code or None on EOF
    async fn read_response<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Option<u16> {
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).await.ok()? == 0 {
                return None;
            }
            if line.len() < 4 {
                return None;
            }
            let bytes = line.as_bytes();
            let code: u16 = std::str::from_utf8(&bytes[0..3]).ok()?.parse().ok()?;
            // '-' means continuation; ' ' means final line.
            if bytes[3] == b' ' {
                return Some(code);
            }
        }
    }

    // greeting
    if read_response(&mut reader).await? != 220 {
        return None;
    }

    // EHLO
    write.write_all(b"EHLO bench.local\r\n").await.ok()?;
    if read_response(&mut reader).await? != 250 {
        return None;
    }

    // MAIL FROM
    write
        .write_all(b"MAIL FROM:<sender@bench.local>\r\n")
        .await
        .ok()?;
    if read_response(&mut reader).await? != 250 {
        return None;
    }

    // RCPT TO
    write
        .write_all(b"RCPT TO:<bob@bench.local>\r\n")
        .await
        .ok()?;
    if read_response(&mut reader).await? != 250 {
        return None;
    }

    // DATA
    write.write_all(b"DATA\r\n").await.ok()?;
    if read_response(&mut reader).await? != 354 {
        return None;
    }

    // body
    write.write_all(body).await.ok()?;
    if read_response(&mut reader).await? != 250 {
        return None;
    }

    // QUIT
    write.write_all(b"QUIT\r\n").await.ok()?;
    let _ = read_response(&mut reader).await;

    Some(start.elapsed())
}

/// Fixed-payload SMTP message body, terminated with `\r\n.\r\n`.
fn build_body() -> Vec<u8> {
    let mut body = String::new();
    body.push_str("From: sender@bench.local\r\n");
    body.push_str("To: bob@bench.local\r\n");
    body.push_str("Subject: bench\r\n");
    body.push_str("Date: Mon, 01 Jan 2030 00:00:00 +0000\r\n");
    body.push_str("Message-ID: <bench@bench.local>\r\n");
    body.push_str("\r\n");
    // ~1 KB payload — representative of a small transactional mail.
    body.push_str(&"x".repeat(1024));
    body.push_str("\r\n.\r\n");
    body.into_bytes()
}

#[derive(Default)]
struct Args {
    duration_s: u64,
    conns: usize,
    warmup_s: u64,
    label: String,
    no_deliver: bool,
}

fn parse_args() -> Args {
    let mut a = Args {
        duration_s: 30,
        conns: 32,
        warmup_s: 3,
        label: String::new(),
        no_deliver: false,
    };
    let argv: Vec<String> = env::args().collect();
    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--duration" => {
                a.duration_s = argv[i + 1].parse().unwrap_or(30);
                i += 2;
            }
            "--conns" => {
                a.conns = argv[i + 1].parse().unwrap_or(32);
                i += 2;
            }
            "--warmup" => {
                a.warmup_s = argv[i + 1].parse().unwrap_or(3);
                i += 2;
            }
            "--label" => {
                a.label = argv[i + 1].clone();
                i += 2;
            }
            "--no-deliver" => {
                a.no_deliver = true;
                i += 1;
            }
            // criterion / cargo bench may pass --bench / test filters when
            // run via `cargo bench`. Ignore them so the binary still works.
            "--bench" | "--nocapture" => {
                i += 1;
            }
            other if other.starts_with("--") => {
                eprintln!("warning: ignoring unknown flag {other}");
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    a
}

fn percentile_us(latencies: &mut [Duration], p: f64) -> u64 {
    if latencies.is_empty() {
        return 0;
    }
    latencies.sort_unstable();
    let idx = ((latencies.len() - 1) as f64 * p).round() as usize;
    latencies[idx].as_micros() as u64
}

async fn run_once(args: &Args) -> Result<(), String> {
    // Per-run maildir under /tmp; cleaned after run.
    let maildir_root = std::env::temp_dir()
        .join(format!("mailrs-bench-{}", std::process::id()))
        .to_string_lossy()
        .to_string();
    let maildir_arc = Arc::new(maildir_root.clone());

    let port = start_server(maildir_arc.clone(), args.no_deliver).await;
    let body = Arc::new(build_body());

    // Warmup — don't measure these, just let the runtime + page cache settle.
    if args.warmup_s > 0 {
        let stop = Arc::new(Notify::new());
        let mut warmup_tasks = Vec::new();
        for _ in 0..args.conns {
            let body = body.clone();
            let stop = stop.clone();
            warmup_tasks.push(tokio::spawn(async move {
                loop {
                    if (tokio::time::timeout(Duration::from_millis(0), stop.notified()).await).is_ok()
                    {
                        return;
                    }
                    let _ = one_session(port, &body).await;
                }
            }));
        }
        tokio::time::sleep(Duration::from_secs(args.warmup_s)).await;
        stop.notify_waiters();
        for t in warmup_tasks {
            // best-effort drain — abort outstanding sessions.
            t.abort();
        }
    }

    // Measured run.
    let stop_at = Instant::now() + Duration::from_secs(args.duration_s);
    let mut tasks = Vec::new();
    for _ in 0..args.conns {
        let body = body.clone();
        tasks.push(tokio::spawn(async move {
            let mut local: Vec<Duration> = Vec::with_capacity(4096);
            while Instant::now() < stop_at {
                if let Some(d) = one_session(port, &body).await {
                    local.push(d);
                }
            }
            local
        }));
    }

    let mut all_latencies: Vec<Duration> = Vec::new();
    for t in tasks {
        match t.await {
            Ok(mut v) => all_latencies.append(&mut v),
            Err(e) => return Err(format!("worker join: {e}")),
        }
    }

    let total_msgs = all_latencies.len();
    let throughput = total_msgs as f64 / args.duration_s as f64;
    let p50 = percentile_us(&mut all_latencies, 0.50);
    let p99 = percentile_us(&mut all_latencies, 0.99);
    let p999 = percentile_us(&mut all_latencies, 0.999);

    // cleanup
    let _ = std::fs::remove_dir_all(&maildir_root);

    // CSV-ish line for scripts to slurp. Also a human header line on first run.
    println!(
        "label={} mode={} duration_s={} conns={} msgs={} throughput_msg_s={:.1} p50_us={} p99_us={} p999_us={}",
        if args.label.is_empty() {
            "_"
        } else {
            args.label.as_str()
        },
        if args.no_deliver { "no-deliver" } else { "deliver" },
        args.duration_s,
        args.conns,
        total_msgs,
        throughput,
        p50,
        p99,
        p999
    );

    Ok(())
}

fn main() -> ExitCode {
    let args = parse_args();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build runtime");
    match rt.block_on(run_once(&args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bench failed: {e}");
            ExitCode::FAILURE
        }
    }
}
