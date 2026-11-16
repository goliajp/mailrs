//! Judge again the mail an observation concerns, as soon as it is added.
//!
//! A reputation observation — an incident, a complaint, a user filing mail
//! as Junk, an administrator's decision — changes what the rules may say
//! about mail already delivered. Each new observation's id is published on
//! `notify:reputation`; this thread hears it, finds the mail from that
//! sender domain or with a link to that host in the fact ledger, judges it
//! again with the rules delivery uses, and records the verdict.
//!
//! **Acting is switched.** What the new verdict would do — hold, release,
//! Junk — is logged and counted. With `MAILRS_REJUDGE_ACT=on` a hold takes
//! the conversation back into Review and tells its reader, and a
//! conversation this module held that the rules no longer hold goes back
//! (`recall`); Junk stays a logged suggestion. One observation holds at
//! most `MAILRS_REJUDGE_HOLD_CAP` conversations (50) on its own; a round
//! that would hold more holds nothing and waits as a pending batch for a
//! super administrator to confirm or discard.
//!
//! **Paged.** An observation that names more mail than one page is judged
//! over several, the place kept in the store, so a restart goes on where it
//! stopped.
//!
//! Event-driven, and it converges: the channel wakes it, and every 30
//! seconds it reads one counter in case an announcement was lost. With no
//! new observation that read is all it does, and it writes nothing.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub mod facts;
pub mod holding;
pub mod judge;
pub mod recall;
mod store;
pub use store::{Conversation, RawReader, RejudgeStore};

/// Where the last observation this thread has dealt with is kept.
const DONE_KEY: &str = "rejudge:obs_seq";

/// The observation being judged over several pages, and how far:
/// `"<seq> <offset>"`. Absent between observations.
pub const PENDING_KEY: &str = "rejudge:pending";

/// How often the counter is read when no announcement came.
const POLL: Duration = Duration::from_secs(30);

/// What the re-judge has done since the process started.
#[derive(Default)]
pub struct Counts {
    pub observations: AtomicU64,
    pub skipped: AtomicU64,
    pub messages: AtomicU64,
    pub no_file: AtomicU64,
    pub changed: AtomicU64,
    pub would_hold: AtomicU64,
    pub would_release: AtomicU64,
    pub would_junk: AtomicU64,
    /// Observations judged over more than one page.
    pub paged: AtomicU64,
    /// Holds and releases actually made.
    pub acted: AtomicU64,
}

pub static COUNTS: std::sync::LazyLock<Counts> = std::sync::LazyLock::new(Counts::default);

impl Counts {
    pub fn add(field: &AtomicU64, n: u64) {
        field.fetch_add(n, Ordering::Relaxed);
    }

    /// For the maintenance route: every counter by name.
    pub fn snapshot(&self) -> serde_json::Value {
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
            "paged": get(&self.paged),
            "acted": get(&self.acted),
            "acting": recall::acting(),
        })
    }
}

/// Start the listener and the re-judge thread. Without a network kevy
/// there are no observations, and nothing to start.
pub fn spawn<S: RejudgeStore>(store: Arc<S>, net_url: Option<String>) {
    let Some(url) = net_url else {
        return;
    };
    let (wake, woken) = std::sync::mpsc::channel::<()>();
    listen(url, wake);
    // a push to the reader's devices is spawned on the runtime
    let runtime = tokio::runtime::Handle::try_current().ok();
    std::thread::Builder::new()
        .name("rejudge".into())
        .spawn(move || {
            let _entered = runtime.as_ref().map(|h| h.enter());
            let mut judge = judge::Judge::new(&*store);
            loop {
                // an announcement or the poll: either way, look
                let _ = woken.recv_timeout(POLL);
                if let Err(e) = tick(&*store, &mut judge) {
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

fn read_done<S: RejudgeStore>(store: &S) -> std::io::Result<Option<u64>> {
    Ok(store
        .state_get(DONE_KEY)?
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|v| v.parse().ok()))
}

fn write_done<S: RejudgeStore>(store: &S, seq: u64) -> std::io::Result<()> {
    store.state_set(DONE_KEY, seq.to_string().as_bytes())
}

fn read_pending<S: RejudgeStore>(store: &S, seq: u64) -> usize {
    store
        .state_get(PENDING_KEY)
        .ok()
        .flatten()
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|v| {
            let (s, o) = v.split_once(' ')?;
            match s.parse::<u64>().ok()? == seq {
                true => o.parse().ok(),
                false => None,
            }
        })
        .unwrap_or(0)
}

fn write_pending<S: RejudgeStore>(store: &S, seq: u64, offset: usize) -> std::io::Result<()> {
    store.state_set(PENDING_KEY, format!("{seq} {offset}").as_bytes())
}

/// One round: judge again for every observation added since the last.
///
/// The first round on a store that has never run starts from the counter
/// as it stands: the observations already there were in force when that
/// mail was last judged. Each observation is marked done as it finishes,
/// and one judged over several pages keeps its place after each, so a
/// restart repeats at most one page. Returns how many messages were
/// judged.
pub fn tick<S: RejudgeStore>(store: &S, judge: &mut judge::Judge) -> std::io::Result<u64> {
    use mailrs_core_sidestate::families::screening;
    let Some(mut conn) = store.net_conn() else {
        return Ok(0);
    };
    let seq = screening::observation_seq(&mut conn)?;
    let Some(done) = read_done(store)? else {
        write_done(store, seq)?;
        return Ok(0);
    };
    if seq <= done {
        return Ok(0);
    }
    judge.refresh(&mut conn);
    let page = judge::page_size();
    let mut judged = 0;
    for n in done + 1..=seq {
        if let Some(o) = screening::observation(&mut conn, &format!("r{n}"))? {
            let from = read_pending(store, n);
            if from == 0 {
                Counts::add(&COUNTS.observations, 1);
            }
            let targets = judge.targets(store, &o).unwrap_or_default();
            if targets.len() > page && from == 0 {
                tracing::info!(id = %o.id, found = targets.len(), page, "rejudge: judged over several pages");
                Counts::add(&COUNTS.paged, 1);
            }
            let mut at = from.min(targets.len());
            while at < targets.len() {
                let end = (at + page).min(targets.len());
                judged += judge.page(store, &targets[at..end], &o.id);
                at = end;
                if at < targets.len() {
                    write_pending(store, n, at)?;
                }
            }
            if from > 0 || targets.len() > page {
                store.state_del(PENDING_KEY)?;
            }
            // what the pages found to hold is held, or waits, only now
            if let Err(e) = recall::settle(store, &o.id) {
                tracing::warn!(error = %e, id = %o.id, "rejudge: the round's holds were not settled");
            }
        }
        write_done(store, n)?;
    }
    Ok(judged)
}
