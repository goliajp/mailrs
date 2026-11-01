//! One IMAP import, run in the background.
//!
//! A user has at most one at a time, and the whole server runs at most
//! [`SLOTS`] at once, the rest waiting their turn, so a few large imports
//! cannot take the disk and the network from mail being delivered. Each
//! start counts towards a daily limit whether it works or not. The
//! password lives in the task and goes with it; what survives a restart is
//! how far the import got, and asking again with the password resumes it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use mailrs_core_sidestate::families::imap_import as progress;
use tokio::sync::Semaphore;

use super::client::{Client, Limits};
use super::connect::Connect;
use super::plan::plan;
use crate::WebState;
use crate::handlers::kevy_util::with_kevy;
use crate::handlers::mbox::format::Parsed;
use crate::handlers::mbox::import::{Filer, Report};

/// Imports running at once, server-wide.
pub const SLOTS: usize = 2;

static RUNNING: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(SLOTS));
static JOBS: LazyLock<Mutex<HashMap<String, Arc<Job>>>> = LazyLock::new(Default::default);

/// Where the import reads from.
pub struct Source {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

/// Where an import stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
    /// Stopped by a restart; asking again with the password resumes it.
    Interrupted,
    None,
}

/// What the settings page shows.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Status {
    pub state: State,
    pub host: String,
    pub port: u16,
    pub username: String,
    /// The folder being read.
    pub folder: Option<String>,
    pub report: Report,
    /// Why it failed, as plain text.
    pub error: Option<String>,
}

impl Status {
    fn none() -> Self {
        Self {
            state: State::None,
            host: String::new(),
            port: 0,
            username: String::new(),
            folder: None,
            report: Report::default(),
            error: None,
        }
    }
}

struct Job {
    status: Mutex<Status>,
    cancel: AtomicBool,
}

impl Job {
    fn update(&self, f: impl FnOnce(&mut Status)) {
        f(&mut self.status.lock().expect("import status lock"));
    }
}

/// Why an import could not start.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    AlreadyRunning,
    TooManyToday,
    Invalid(String),
}

fn today() -> String {
    chrono::Utc::now().format("%Y%m%d").to_string()
}

/// Start `user`'s import from `source`, reaching it through `connect`.
pub fn start(
    state: Arc<WebState>,
    user: &str,
    source: Source,
    connect: Arc<dyn Connect>,
) -> Result<Status, Refused> {
    let mut jobs = JOBS.lock().expect("import jobs lock");
    if let Some(j) = jobs.get(user) {
        let s = j.status.lock().expect("import status lock").state;
        if matches!(s, State::Queued | State::Running) {
            return Err(Refused::AlreadyRunning);
        }
    }
    let (u, day) = (user.to_string(), today());
    let started = with_kevy(move |c| progress::count_start(c, &u, &day))
        .map_err(|_| Refused::Invalid("the import could not be recorded".into()))?;
    if started > progress::STARTS_PER_DAY {
        return Err(Refused::TooManyToday);
    }
    if !super::connect::PORTS.contains(&source.port) {
        return Err(Refused::Invalid("only ports 993 and 143".into()));
    }
    // resume only the same source; a different one starts over
    let u = user.to_string();
    let (host, port, name) = (source.host.clone(), source.port, source.username.clone());
    let resume = with_kevy(move |c| {
        let before = progress::progress(c, &u)?;
        match before {
            Some(p) if p.host == host && p.port == port && p.username == name => Ok(p.last_uid),
            _ => {
                progress::begin(c, &u, &host, port, &name)?;
                Ok(HashMap::new())
            }
        }
    })
    .map_err(|_| Refused::Invalid("the import could not be recorded".into()))?;
    let status = Status {
        state: State::Queued,
        host: source.host.clone(),
        port: source.port,
        username: source.username.clone(),
        ..Status::none()
    };
    let job = Arc::new(Job {
        status: Mutex::new(status.clone()),
        cancel: AtomicBool::new(false),
    });
    jobs.insert(user.to_string(), job.clone());
    tokio::spawn(run(state, user.to_string(), source, connect, job, resume));
    Ok(status)
}

/// Where `user`'s import stands, the one running or the one a restart cut
/// short.
pub fn status(user: &str) -> Status {
    if let Some(j) = JOBS.lock().expect("import jobs lock").get(user) {
        return j.status.lock().expect("import status lock").clone();
    }
    let u = user.to_string();
    match with_kevy(move |c| progress::progress(c, &u)) {
        Ok(Some(p)) => Status {
            state: State::Interrupted,
            host: p.host,
            port: p.port,
            username: p.username,
            ..Status::none()
        },
        _ => Status::none(),
    }
}

/// Ask `user`'s import to stop after the message in hand.
pub fn cancel(user: &str) -> bool {
    match JOBS.lock().expect("import jobs lock").get(user) {
        Some(j) => {
            j.cancel.store(true, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

async fn run(
    state: Arc<WebState>,
    user: String,
    source: Source,
    connect: Arc<dyn Connect>,
    job: Arc<Job>,
    resume: HashMap<String, u32>,
) {
    let _slot = RUNNING
        .acquire()
        .await
        .expect("the import semaphore is never closed");
    job.update(|s| s.state = State::Running);
    let mut filer = Filer::new(state, user.clone()).await;
    let outcome = import(&user, &source, connect.as_ref(), &job, &resume, &mut filer).await;
    filer.report.complete = outcome.is_ok();
    let (state_after, error) = match outcome {
        Ok(()) => (State::Done, None),
        Err(Stop::Cancelled) => (State::Cancelled, None),
        Err(Stop::Failed(e)) => (State::Failed, Some(e)),
    };
    if state_after == State::Done {
        let u = user.clone();
        if let Err(e) = with_kevy(move |c| progress::finish(c, &u)) {
            tracing::warn!(%user, ?e, "imap import: the finished import's progress was not cleared");
        }
    }
    let r = &filer.report;
    crate::handlers::audit::record(
        &user,
        "mail.import.imap",
        &source.host,
        &format!(
            "outcome={state_after:?} imported={} sent={} archived={} held={} duplicate={} junk_or_trash={} too_large={} failed={}",
            r.imported,
            r.sent,
            r.archived,
            r.held,
            r.skipped_duplicate,
            r.skipped_junk_or_trash,
            r.too_large,
            r.failed
        ),
    );
    job.update(|s| {
        s.state = state_after;
        s.error = error;
        s.folder = None;
        s.report = filer.report.clone();
    });
}

enum Stop {
    Cancelled,
    Failed(String),
}

impl From<super::client::Error> for Stop {
    fn from(e: super::client::Error) -> Self {
        Stop::Failed(e.to_string())
    }
}

async fn import(
    user: &str,
    source: &Source,
    connect: &dyn Connect,
    job: &Job,
    resume: &HashMap<String, u32>,
    filer: &mut Filer,
) -> Result<(), Stop> {
    let opened = connect
        .connect(&source.host, source.port)
        .await
        .map_err(Stop::Failed)?;
    let limits = Limits::default();
    let mut client = Client::open(opened.stream, limits, opened.greeted).await?;
    // one try: a failed sign-in ends the import with the server's reason
    client.login(&source.username, &source.password).await?;
    for (name, folder) in plan(client.list().await?) {
        job.update(|s| s.folder = Some(name.clone()));
        client.examine(&name).await?;
        let from = resume.get(&name).map_or(1, |u| u.saturating_add(1));
        for listed in client.messages_from(from).await? {
            if job.cancel.load(Ordering::Relaxed) {
                client.logout().await;
                return Err(Stop::Cancelled);
            }
            let keep_going = match listed.size > u64::from(limits.max_message) {
                true => {
                    filer.report.too_large += 1;
                    true
                }
                false => {
                    let raw = client.fetch(listed.uid).await?;
                    filer
                        .file(Parsed {
                            raw,
                            folder,
                            seen: listed.seen,
                        })
                        .await
                }
            };
            let (u, n, uid) = (user.to_string(), name.clone(), listed.uid);
            if let Err(e) = with_kevy(move |c| progress::record(c, &u, &n, uid)) {
                tracing::warn!(%user, ?e, "imap import: progress not recorded");
            }
            job.update(|s| s.report = filer.report.clone());
            if !keep_going {
                client.logout().await;
                return Err(Stop::Failed("the mailbox is over its quota".into()));
            }
        }
    }
    client.logout().await;
    Ok(())
}
