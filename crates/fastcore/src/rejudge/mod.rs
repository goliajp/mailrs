//! Judge again the mail an observation concerns, as soon as it is added.
//!
//! A reputation observation — an incident, a complaint, a user filing mail
//! as Junk, an administrator's decision — changes what the rules may say
//! about mail already delivered. Each new observation's id is published on
//! `notify:reputation`; this thread hears it, finds the mail from that
//! sender domain or with a link to that host in the fact ledger, judges it
//! again with the rules delivery uses, and records the verdict.
//!
//! **Shadow.** What the new verdict would do — hold, release, Junk — is
//! logged and counted and nothing is moved. Acting waits until a week of
//! these counts has been read.
//!
//! Event-driven, and it converges: the channel wakes it, and every 30
//! seconds it reads one counter in case an announcement was lost. With no
//! new observation that read is all it does, and it writes nothing.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::FastcoreState;

mod judge;

/// Where the last observation this thread has dealt with is kept, in
/// fastcore's own store.
const DONE_KEY: &[u8] = b"rejudge:obs_seq";

/// How often the counter is read when no announcement came.
const POLL: Duration = Duration::from_secs(30);

/// What the re-judge has done since the process started.
#[derive(Default)]
pub(crate) struct Counts {
    pub observations: AtomicU64,
    pub skipped: AtomicU64,
    pub messages: AtomicU64,
    pub no_file: AtomicU64,
    pub changed: AtomicU64,
    pub would_hold: AtomicU64,
    pub would_release: AtomicU64,
    pub would_junk: AtomicU64,
    pub over_cap: AtomicU64,
}

pub(crate) static COUNTS: std::sync::LazyLock<Counts> = std::sync::LazyLock::new(Counts::default);

impl Counts {
    pub(crate) fn add(field: &AtomicU64, n: u64) {
        field.fetch_add(n, Ordering::Relaxed);
    }

    /// For the maintenance route: every counter by name.
    pub(crate) fn snapshot(&self) -> serde_json::Value {
        let get = |f: &AtomicU64| f.load(Ordering::Relaxed);
        serde_json::json!({
            "observations": get(&self.observations),
            "skipped": get(&self.skipped),
            "messages_judged": get(&self.messages),
            "no_file": get(&self.no_file),
            "verdicts_changed": get(&self.changed),
            "would_hold": get(&self.would_hold),
            "would_release": get(&self.would_release),
            "would_junk": get(&self.would_junk),
            "over_cap": get(&self.over_cap),
        })
    }
}

/// `GET /v1/admin/maintenance:rejudge-status`: what the re-judge has done
/// since the process started, and what it would have done. Read-only.
pub(crate) async fn status_route() -> axum::Json<serde_json::Value> {
    axum::Json(COUNTS.snapshot())
}

/// `POST /v1/admin/maintenance:rejudge-now`: one round now rather than at
/// the next announcement or poll, for an operator timing the path on a
/// copy. Shadow like every round: it records verdicts and moves nothing.
pub(crate) async fn now_route(
    axum::extract::State(state): axum::extract::State<Arc<FastcoreState>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let round = tokio::task::spawn_blocking(move || {
        let mut judge = judge::Judge::new(&state);
        tick(&state, &mut judge)
    })
    .await;
    match round {
        Ok(Ok(judged)) => axum::Json(serde_json::json!({
            "judged": judged,
            "since_start": COUNTS.snapshot(),
        }))
        .into_response(),
        Ok(Err(e)) => (axum::http::StatusCode::SERVICE_UNAVAILABLE, e.to_string()).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Start the listener and the re-judge thread. Without a network kevy
/// there are no observations, and nothing to start.
pub(crate) fn spawn(state: Arc<FastcoreState>) {
    let Some(url) = state.net_url.clone() else {
        return;
    };
    let (wake, woken) = std::sync::mpsc::channel::<()>();
    listen(url, wake);
    std::thread::Builder::new()
        .name("rejudge".into())
        .spawn(move || {
            let mut judge = judge::Judge::new(&state);
            loop {
                // an announcement or the poll: either way, look
                let _ = woken.recv_timeout(POLL);
                if let Err(e) = tick(&state, &mut judge) {
                    tracing::warn!(error = %e, "rejudge: this round failed; the next one retries");
                }
            }
        })
        .expect("spawn rejudge thread");
}

/// Wake the re-judge on every announced observation; resubscribe after a
/// dropped connection.
fn listen(url: String, wake: std::sync::mpsc::Sender<()>) {
    use mailrs_core_sidestate::families::screening::REPUTATION_CHANNEL;
    std::thread::Builder::new()
        .name("rejudge-wake".into())
        .spawn(move || {
            loop {
                match kevy_client::Subscriber::connect_channels(&url, &[REPUTATION_CHANNEL]) {
                    Ok(mut sub) => {
                        while sub.recv_message().is_ok() {
                            if wake.send(()).is_err() {
                                return;
                            }
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "rejudge: subscribe failed; retry in 1s"),
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        })
        .expect("spawn rejudge-wake thread");
}

fn read_done(state: &FastcoreState) -> std::io::Result<Option<u64>> {
    Ok(state
        .mailbox
        .store_ref()
        .get(DONE_KEY)?
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|v| v.parse().ok()))
}

fn write_done(state: &FastcoreState, seq: u64) -> std::io::Result<()> {
    state
        .mailbox
        .store_ref()
        .set(DONE_KEY, seq.to_string().as_bytes())?;
    Ok(())
}

/// One round: judge again for every observation added since the last.
///
/// The first round on a store that has never run starts from the counter
/// as it stands: the observations already there were in force when that
/// mail was last judged. Returns how many messages were judged.
pub(crate) fn tick(state: &Arc<FastcoreState>, judge: &mut judge::Judge) -> std::io::Result<u64> {
    use mailrs_core_sidestate::families::screening;
    let Some(mut conn) = state.net_conn() else {
        return Ok(0);
    };
    let seq = screening::observation_seq(&mut conn)?;
    let Some(done) = read_done(state)? else {
        write_done(state, seq)?;
        return Ok(0);
    };
    if seq <= done {
        return Ok(0);
    }
    judge.refresh(&mut conn);
    let mut judged = 0;
    for n in done + 1..=seq {
        let Some(o) = screening::observation(&mut conn, &format!("r{n}"))? else {
            continue;
        };
        Counts::add(&COUNTS.observations, 1);
        judged += judge.observation(state, &o);
    }
    write_done(state, seq)?;
    Ok(judged)
}
