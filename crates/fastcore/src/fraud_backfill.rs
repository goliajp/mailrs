//! Durable, bounded reconciliation after a Lua rule version changes.
use std::{fs, io::Write, path::Path, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    FastcoreState,
    maintenance::{fraud_rescan, hangup::Hangup},
};

const BATCH: usize = 100;

#[derive(Debug, Serialize, Deserialize)]
struct Progress {
    version: String,
    cursor: usize,
    total: usize,
    held: u64,
    released: u64,
    /// Not held, scored into Junk. `default` so a journal written
    /// before this field existed still reads.
    #[serde(default)]
    junked: u64,
    no_file: u64,
    complete: bool,
    /// Conversations skipped because the process died judging them.
    #[serde(default)]
    skipped: u64,
}

/// A batch started and not yet saved as done: `inflight.json`. Found at
/// the same cursor when the next run starts, it says the process died
/// during that batch.
#[derive(Debug, Serialize, Deserialize)]
struct Inflight {
    version: String,
    cursor: usize,
    end: usize,
    /// Starts of this same range that did not finish.
    attempts: u32,
    /// Set once a batch was interrupted [`BATCH_ATTEMPTS`] times: until the
    /// cursor reaches this, conversations are judged one at a time.
    #[serde(default)]
    one_by_one_until: Option<usize>,
}

/// Interrupted starts of one batch before it is judged one conversation at
/// a time. As for the spool: a deploy can interrupt an innocent batch once.
const BATCH_ATTEMPTS: u32 = 3;

/// Interrupted starts of one conversation, judged alone, before it is
/// skipped. One: its batch has already died [`BATCH_ATTEMPTS`] times, and
/// a skipped conversation is recorded in `skipped.json` for a manual
/// rescan, so a wrong skip is cheap.
const ITEM_ATTEMPTS: u32 = 1;

/// One conversation the backfill skipped.
#[derive(Debug, Serialize, Deserialize)]
struct Skipped {
    version: String,
    user: String,
    thread_id: String,
    at: i64,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    match fs::read(path) {
        Ok(raw) => serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// How many conversations the backfill in `journal` skipped for its
/// current version; `None` when it has not started.
pub fn skipped(journal: &Path) -> Option<u64> {
    read_json::<Progress>(&journal.join("progress.json"))
        .ok()
        .flatten()
        .map(|p| p.skipped)
}

/// The range to judge next from `progress.cursor`, and the in-flight
/// record to write before judging it. Skips (and records) the conversation
/// a run judging it alone died on.
fn next_range(
    journal: &Path,
    progress: &mut Progress,
    targets: &[(String, String)],
) -> Result<Inflight, String> {
    let flight = read_json::<Inflight>(&journal.join("inflight.json"))?
        .filter(|f| f.version == progress.version && f.cursor == progress.cursor);
    let mut until = flight.as_ref().and_then(|f| f.one_by_one_until);
    let mut prior = flight.as_ref().map_or(0, |f| f.attempts);
    match &flight {
        Some(f) if f.attempts >= ITEM_ATTEMPTS && f.end == f.cursor + 1 && until.is_some() => {
            let (user, thread_id) = targets[f.cursor].clone();
            tracing::error!(event = "fraud_backfill_skipped", version = %f.version,
                cursor = f.cursor, %user, %thread_id,
                "fraud backfill: the process stopped judging this conversation; skipped");
            let skipped_file = journal.join("skipped.json");
            let mut list: Vec<Skipped> = read_json(&skipped_file)?.unwrap_or_default();
            list.push(Skipped {
                version: f.version.clone(),
                user,
                thread_id,
                at: now(),
            });
            save(&skipped_file, &list)?;
            progress.cursor += 1;
            progress.skipped += 1;
            progress.complete = progress.cursor == progress.total;
            save(&journal.join("progress.json"), progress)?;
            prior = 0;
        }
        Some(f) if f.attempts >= BATCH_ATTEMPTS && until.is_none() => {
            tracing::warn!(version = %f.version, cursor = f.cursor, end = f.end,
                "fraud backfill: this batch stopped the process {} times; judging it one at a time",
                f.attempts);
            until = Some(f.end);
            prior = 0;
        }
        Some(f) if f.attempts > 0 => {
            tracing::warn!(version = %f.version, cursor = f.cursor, attempts = f.attempts,
                "fraud backfill: the last run stopped during this batch");
        }
        _ => {}
    }
    let until = until.filter(|u| progress.cursor < *u);
    let end = match until {
        Some(_) => progress.cursor + 1,
        None => progress.cursor + BATCH,
    }
    .min(targets.len());
    Ok(Inflight {
        version: progress.version.clone(),
        cursor: progress.cursor,
        end,
        attempts: prior + 1,
        one_by_one_until: until,
    })
}

fn save(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, value).map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())?;
    fs::File::open(path.parent().ok_or("missing journal parent")?)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| e.to_string())
}

fn version(source: &str) -> String {
    format!("lua:{:x}", Sha256::digest(source.as_bytes()))
}

fn validate(source: &str) -> Result<(), String> {
    let mut rules = mailrs_fraud_lua::Rules::compile(source)?;
    let probe = rules.classify(&Default::default(), &Default::default());
    if probe.errors.is_empty() {
        Ok(())
    } else {
        Err(format!("invalid rules: {:?}", probe.errors))
    }
}

/// Run at most 100 conversations, or do no work for a completed version.
/// The journal and snapshot are durable; only one caller may own this directory.
///
/// All of it runs on the blocking pool: the snapshot, the journal's fsyncs
/// and every judgement are synchronous, and on a runtime worker they stall
/// whatever request is queued behind that worker.
pub async fn run_once(
    state: Arc<FastcoreState>,
    journal: &Path,
    rules_file: &Path,
) -> Result<bool, String> {
    let (journal, rules_file) = (journal.to_path_buf(), rules_file.to_path_buf());
    crate::maintenance::hangup::run_blocking(move |h| {
        run_blocking(&state, &journal, &rules_file, h)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn run_blocking(
    state: &Arc<FastcoreState>,
    journal: &Path,
    rules_file: &Path,
    hangup: &Hangup,
) -> Result<bool, String> {
    use std::io::Read;
    let mut source = String::new();
    fs::File::open(rules_file)
        .and_then(|f| {
            f.take((mailrs_fraud_lua::MAX_SOURCE + 1) as u64)
                .read_to_string(&mut source)
        })
        .map_err(|e| e.to_string())?;
    if source.len() > mailrs_fraud_lua::MAX_SOURCE {
        return Err("rules file too large".into());
    }
    let current = version(&source);
    let progress_file = journal.join("progress.json");
    let mut progress: Option<Progress> = match fs::read(&progress_file) {
        Ok(raw) => Some(serde_json::from_slice(&raw).map_err(|e| e.to_string())?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    let snapshot_file = journal.join(format!("snapshot-{}.json", &current[4..]));
    if progress.as_ref().is_none_or(|p| p.version != current) {
        validate(&source)?;
        let targets = fraud_rescan::snapshot(state).map_err(|e| e.to_string())?;
        fs::create_dir_all(journal).map_err(|e| e.to_string())?;
        // Snapshot first, then publish its cursor. No offset into a changing
        // keyspace, and no reshuffling on restart or arrival of new mail.
        save(&snapshot_file, &targets)?;
        let next = Progress {
            version: current.clone(),
            cursor: 0,
            total: targets.len(),
            held: 0,
            released: 0,
            junked: 0,
            no_file: 0,
            complete: false,
            skipped: 0,
        };
        save(&progress_file, &next)?;
        if let Some(old) = &progress {
            let old_file = journal.join(format!("snapshot-{}.json", &old.version[4..]));
            if old_file != snapshot_file {
                let _ = fs::remove_file(old_file);
            }
        }
        tracing::info!(version=%current, total=next.total, "fraud backfill queued");
        progress = Some(next);
    }
    let mut progress = progress.ok_or("missing progress")?;
    if progress.complete {
        return Ok(false);
    }
    let targets: Vec<(String, String)> =
        serde_json::from_slice(&fs::read(&snapshot_file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if targets.len() != progress.total || progress.cursor > progress.total {
        return Err("backfill snapshot/cursor mismatch".into());
    }
    let inflight_file = journal.join("inflight.json");
    let flight = next_range(journal, &mut progress, &targets)?;
    if progress.complete {
        let _ = fs::remove_file(&inflight_file);
        return Ok(false);
    }
    // on disk before judging, so a process that dies judging leaves it
    save(&inflight_file, &flight)?;
    let end = flight.end;
    let batch = targets[progress.cursor..end].to_vec();
    let judged = judge(state, batch, &current, &mut progress, end, hangup);
    if let Err(e) = &judged {
        // a run that returned did not die: this start is not counted
        let undo = Inflight {
            attempts: flight.attempts - 1,
            ..flight
        };
        save(&inflight_file, &undo)?;
        return Err(e.clone());
    }
    progress.complete = end == progress.total;
    save(&progress_file, &progress)?;
    match flight.one_by_one_until.filter(|u| end < *u) {
        Some(until) => save(
            &inflight_file,
            &Inflight {
                version: current.clone(),
                cursor: end,
                end: end + 1,
                attempts: 0,
                one_by_one_until: Some(until),
            },
        )?,
        None => fs::remove_file(&inflight_file).map_err(|e| e.to_string())?,
    }
    tracing::info!(version=%current, cursor=end, total=progress.total,
        complete=progress.complete, held=progress.held, released=progress.released,
        junked=progress.junked, no_file=progress.no_file, skipped=progress.skipped,
        "fraud backfill progress");
    Ok(!progress.complete)
}

/// Judge `batch`, the targets up to `end`, and add what came of it to
/// `progress` with the cursor moved to `end`.
fn judge(
    state: &Arc<FastcoreState>,
    batch: Vec<(String, String)>,
    current: &str,
    progress: &mut Progress,
    end: usize,
    hangup: &Hangup,
) -> Result<(), String> {
    // A lost response/checkpoint repeats only this batch. The apply path checks
    // existing verdict, quarantine and unread state before writing.
    let result = fraud_rescan::walk(
        state,
        &fraud_rescan::RescanQuery {
            dry_run: false,
            skip: 0,
            limit: BATCH as u64,
            pause_ms: 100,
            action: fraud_rescan::Action::Hold,
        },
        Some(batch),
        Some(current),
        hangup,
    )
    .map_err(|e| format!("rescan failed: {e}"))?;
    if result["verdict_failed"].as_u64() != Some(0)
        || result["threads_walked"].as_u64() != Some((end - progress.cursor) as u64)
    {
        return Err(format!("batch incomplete; cursor retained: {result}"));
    }
    progress.cursor = end;
    progress.held += result["held"].as_u64().ok_or("missing held count")?;
    progress.released += result["released"]
        .as_u64()
        .ok_or("missing released count")?;
    progress.junked += result["scored_moved_to_junk"]
        .as_u64()
        .ok_or("missing scored_moved_to_junk count")?;
    progress.no_file += result["no_file"].as_u64().ok_or("missing no_file count")?;
    Ok(())
}

/// The journal of the backfill this process runs; unset when it runs none.
static JOURNAL: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// [`skipped`] for the backfill this process runs.
pub(crate) fn skipped_here() -> Option<u64> {
    skipped(JOURNAL.get()?)
}

pub(crate) fn spawn(state: Arc<FastcoreState>, data_dir: &str) {
    let Some(rules) = std::env::var_os("MAILRS_FRAUD_RULES_FILE") else {
        return;
    };
    let rules = std::path::PathBuf::from(rules);
    let journal = Path::new(data_dir).join("fraud-backfill");
    let _ = JOURNAL.set(journal.clone());
    tokio::spawn(async move {
        let mut delay = 15;
        loop {
            tokio::time::sleep(Duration::from_secs(delay)).await;
            match run_once(state.clone(), &journal, &rules).await {
                Ok(true) => delay = 2,
                Ok(false) => delay = 30,
                Err(error) => {
                    delay = (delay * 2).clamp(10, 300);
                    tracing::error!(%error, retry_secs=delay, "fraud backfill paused; will retry");
                }
            }
        }
    });
}
