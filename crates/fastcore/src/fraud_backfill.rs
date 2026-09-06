//! Durable, bounded reconciliation after a Lua rule version changes.
use std::{fs, io::Write, path::Path, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{FastcoreState, maintenance::fraud_rescan};

const BATCH: usize = 100;

#[derive(Debug, Serialize, Deserialize)]
struct Progress {
    version: String,
    cursor: usize,
    total: usize,
    held: u64,
    released: u64,
    no_file: u64,
    complete: bool,
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
pub async fn run_once(
    state: Arc<FastcoreState>,
    journal: &Path,
    rules_file: &Path,
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
        let targets = fraud_rescan::snapshot(&state).map_err(|e| e.to_string())?;
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
            no_file: 0,
            complete: false,
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
    let end = (progress.cursor + BATCH).min(targets.len());
    let batch = targets[progress.cursor..end].to_vec();
    // A lost response/checkpoint repeats only this batch. The apply path checks
    // existing verdict, quarantine and unread state before writing.
    let response = fraud_rescan::rescan(
        state,
        fraud_rescan::RescanQuery {
            dry_run: false,
            skip: 0,
            limit: BATCH as u64,
            pause_ms: 100,
            action: fraud_rescan::Action::Hold,
        },
        Some(batch),
        Some(&current),
    )
    .await;
    if !response.status().is_success() {
        return Err(format!("rescan failed: {}", response.status()));
    }
    let bytes = axum::body::to_bytes(response.into_body(), 128 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    let result: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
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
    progress.no_file += result["no_file"].as_u64().ok_or("missing no_file count")?;
    progress.complete = end == progress.total;
    save(&progress_file, &progress)?;
    tracing::info!(version=%current, cursor=end, total=progress.total,
        complete=progress.complete, held=progress.held, released=progress.released,
        no_file=progress.no_file, "fraud backfill progress");
    Ok(!progress.complete)
}

pub(crate) fn spawn(state: Arc<FastcoreState>, data_dir: &str) {
    let Some(rules) = std::env::var_os("MAILRS_FRAUD_RULES_FILE") else {
        return;
    };
    let rules = std::path::PathBuf::from(rules);
    let journal = Path::new(data_dir).join("fraud-backfill");
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
