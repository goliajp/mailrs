//! The in-process SMTP peer the load bench drives: its line codec and
//! the per-connection state machine.

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
//! Kevy writes). Those need a full integration environment (Postgres,
//! Kevy, DNS) and produce variance much larger than the LTO delta we
//! are trying to detect. Treat the numbers from this bench as a *lower
//! bound* on the LTO impact — the real server has more cross-crate
//! inline opportunities in its hot path.
//!
//! Where to bench the pieces excluded here:
//!
//! - **Inbound pipeline framework dispatch + final-decision policy** —
//!   `cargo bench -p mailrs-inbound` (criterion suites
//!   `decision` + `pipeline`). Covers `Pipeline::run` overhead with
//!   N no-op stages and `make_delivery_decision` /
//!   `format_auth_results_header` hot paths in isolation, no PG/Kevy.
//! - **PG / Kevy end-to-end** — intentionally NOT a criterion bench.
//!   Per-call variance from network + WAL fsync swamps the CPU-side
//!   regressions a microbench is supposed to catch. Use the integration
//!   harness in `crates/server/tests/` against a docker-compose'd
//!   Postgres + Kevy instead, and gate on throughput in CI rather
//!   than as a criterion benchmark.
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

use std::sync::Arc;

use bytes::{Buf, BytesMut};
use futures_util::{SinkExt, StreamExt};
use mailrs_delivery_executor::DeliveryExecutor;
use mailrs_smtp_proto::response::{Response, format_ehlo_response};
use mailrs_smtp_proto::session::{Event, Session, SessionConfig};
use mailrs_smtp_proto::{parse_command, unstuff_data};
use tokio::net::{TcpListener, TcpStream};
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

async fn handle_connection(
    stream: TcpStream,
    maildir_root: Arc<String>,
    no_deliver: bool,
    executor: Arc<DeliveryExecutor>,
) {
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
                            if framed.send(Response::data_start().format()).await.is_err() {
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
                                    // Deliveries routed through
                                    // mailrs-delivery-executor
                                    // (group-commit on top of
                                    // maildir 1.2 deliver_batch).
                                    // At saturation (32 conns
                                    // delivering to the same path)
                                    // batches fill to max_batch=64
                                    // and unlock the 15× microbench
                                    // speedup.
                                    let body_arc = std::sync::Arc::new(body);
                                    for rcpt in &forward_paths {
                                        if let Some((local, domain)) = rcpt.split_once('@') {
                                            let path = format!(
                                                "{}/{domain}/{local}",
                                                maildir_root.as_str()
                                            );
                                            if executor
                                                .deliver(path, body_arc.clone())
                                                .await
                                                .is_err()
                                            {
                                                ok = false;
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
                            if framed
                                .send(Response::bad_sequence().format())
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                }
                Err(_) => {
                    if framed
                        .send(Response::syntax_error().format())
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            },
            Input::Data(_) => return,
        }
    }
}

pub(crate) async fn start_server(
    maildir_root: Arc<String>,
    no_deliver: bool,
    executor: Arc<DeliveryExecutor>,
) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let root = maildir_root.clone();
                    let exec = executor.clone();
                    tokio::spawn(
                        async move { handle_connection(stream, root, no_deliver, exec).await },
                    );
                }
                Err(_) => return,
            }
        }
    });
    port
}

// ----- client driver -----
