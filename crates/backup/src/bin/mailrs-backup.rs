//! `mailrs-backup` — the daily backup service, and its restore.
//!
//!   mailrs-backup                                 run daily at MAILRS_BACKUP_AT (UTC)
//!   mailrs-backup run                             one run now
//!   mailrs-backup restore <archive> <identity> <dir>
//!
//! Every run is audited (`backup.done` / `backup.failed`); a failure, or
//! no success for 36 hours, raises a `BackupFailed` security alert. Both
//! go through the network kevy at `MAILRS_KEVY_URL`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mailrs_auth_guard::alerts::{Alert, Fact, Kind};
use mailrs_backup::{Outcome, Plan, Source, SourceKind};

const DEFAULT_SOURCES: &str = "kevy-fastcore=kevy:/data/kevy-fastcore,maildir=maildir:/data/maildir,kevy-server=kevy:/kevy-server,dkim=files:/dkim";

/// No success for this long is reported even when no run failed outright.
const STALE_SECS: i64 = 36 * 3600;

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    mailrs_core_api::refuse_blank_secrets();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["restore", archive, identity, into] => {
            match mailrs_backup::restore(Path::new(archive), Path::new(identity), Path::new(into)) {
                Ok(()) => {
                    println!("restored into {into}");
                    std::process::ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("restore failed: {e}");
                    std::process::ExitCode::FAILURE
                }
            }
        }
        ["run"] => match plan() {
            Ok(plan) => match once(&plan).await {
                Some(o) if o.ok => std::process::ExitCode::SUCCESS,
                _ => std::process::ExitCode::FAILURE,
            },
            Err(e) => config_error(&e),
        },
        [] => match (plan(), run_at()) {
            (Ok(plan), Ok(at)) => daemon(plan, at).await,
            (Err(e), _) | (_, Err(e)) => config_error(&e),
        },
        _ => {
            eprintln!("usage: mailrs-backup [run | restore <archive> <identity file> <empty dir>]");
            std::process::ExitCode::from(2)
        }
    }
}

fn config_error(e: &str) -> std::process::ExitCode {
    eprintln!("{e}");
    std::process::ExitCode::from(2)
}

fn plan() -> Result<Plan, String> {
    let recipient = std::env::var("MAILRS_BACKUP_AGE_RECIPIENT")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or(
            "MAILRS_BACKUP_AGE_RECIPIENT is required: the age public key archives are encrypted to",
        )?;
    let keep = match std::env::var("MAILRS_BACKUP_KEEP") {
        Err(_) => 2,
        Ok(v) => v
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or(format!(
                "MAILRS_BACKUP_KEEP={v:?}: expected a positive number"
            ))?,
    };
    let raw = std::env::var("MAILRS_BACKUP_SOURCES").unwrap_or_else(|_| DEFAULT_SOURCES.into());
    Ok(Plan {
        sources: parse_sources(&raw)?,
        target: PathBuf::from(
            std::env::var("MAILRS_BACKUP_TARGET").unwrap_or_else(|_| "/backup".into()),
        ),
        recipient,
        keep,
    })
}

/// `name=kind:path,...`
fn parse_sources(raw: &str) -> Result<Vec<Source>, String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|entry| {
            let (name, rest) = entry
                .split_once('=')
                .ok_or(format!("{entry:?}: expected name=kind:path"))?;
            let (kind, path) = rest
                .split_once(':')
                .ok_or(format!("{entry:?}: expected name=kind:path"))?;
            let kind = match kind {
                "kevy" => SourceKind::Kevy,
                "maildir" => SourceKind::Maildir,
                "files" => SourceKind::Files,
                other => return Err(format!("{entry:?}: unknown kind {other:?}")),
            };
            Ok(Source {
                name: name.to_string(),
                path: PathBuf::from(path),
                kind,
            })
        })
        .collect()
}

/// `HH:MM` UTC from `MAILRS_BACKUP_AT`, 17:00 by default.
fn run_at() -> Result<(u32, u32), String> {
    let raw = std::env::var("MAILRS_BACKUP_AT").unwrap_or_else(|_| "17:00".into());
    let parsed = raw
        .split_once(':')
        .and_then(|(h, m)| Some((h.trim().parse::<u32>().ok()?, m.trim().parse::<u32>().ok()?)))
        .filter(|(h, m)| *h < 24 && *m < 60);
    parsed.ok_or(format!("MAILRS_BACKUP_AT={raw:?}: expected HH:MM in UTC"))
}

async fn daemon(plan: Plan, at: (u32, u32)) -> std::process::ExitCode {
    tracing::info!(target = %plan.target.display(), at = ?at, keep = plan.keep, "backups scheduled");
    let mut last_day = String::new();
    loop {
        let now = chrono::Utc::now();
        let day = now.format("%Y%m%d").to_string();
        let due = now.format("%H:%M").to_string() >= format!("{:02}:{:02}", at.0, at.1);
        let done_today = mailrs_backup::last_outcome(&plan.target)
            .is_some_and(|o| o.file == format!("mailrs-{day}.tar.age") && o.ok);
        if due && !done_today && last_day != day {
            last_day = day;
            once(&plan).await;
        }
        check_stale(&plan.target).await;
        tokio::time::sleep(Duration::from_secs(300)).await;
    }
}

/// One run, audited, with an alert when it failed.
async fn once(plan: &Plan) -> Option<Outcome> {
    let now = chrono::Utc::now();
    let day = now.format("%Y%m%d").to_string();
    let p = plan.clone();
    let res = tokio::task::spawn_blocking(move || {
        mailrs_backup::run(&p, now.timestamp(), &day, free_bytes)
    })
    .await;
    let outcome = match res {
        Ok(Ok(o)) => o,
        other => {
            let reason = format!("{other:?}");
            tracing::error!(%reason, "backup could not record its outcome");
            let detail = format!("the run could not record its outcome: {reason}");
            alert(
                &detail,
                vec![Fact::Note {
                    text: detail.clone(),
                }],
            )
            .await;
            return None;
        }
    };
    let detail = match outcome.ok {
        true => format!(
            "{} bytes={} sha256={}",
            outcome.file, outcome.bytes, outcome.sha256
        ),
        false => format!("{}: {}", outcome.file, outcome.reason),
    };
    let action = match outcome.ok {
        true => "backup.done",
        false => "backup.failed",
    };
    tracing::info!(ok = outcome.ok, %detail, "backup run");
    audit(action, &detail).await;
    if !outcome.ok {
        let note = Fact::Note {
            text: outcome.reason.clone(),
        };
        alert(&outcome.reason, vec![note]).await;
    }
    Some(outcome)
}

async fn check_stale(target: &Path) {
    let now = chrono::Utc::now().timestamp();
    let Some(last) = mailrs_backup::last_outcome(target) else {
        return;
    };
    let since = last.last_success.unwrap_or(last.started);
    if now - since > STALE_SECS {
        let hours = (now - since) / 3600;
        alert(
            &format!("no backup has succeeded for {hours} hours"),
            vec![Fact::NoBackupFor {
                hours: hours.max(0) as u64,
            }],
        )
        .await;
    }
}

async fn alert(detail: &str, facts: Vec<Fact>) {
    let Ok(url) = std::env::var("MAILRS_KEVY_URL") else {
        tracing::error!(%detail, "backup alert not raised: MAILRS_KEVY_URL unset");
        return;
    };
    let alert = Alert {
        kind: Kind::BackupFailed,
        subject: hostname(),
        at: chrono::Utc::now().timestamp().max(0) as u64,
        detail: detail.to_string(),
        facts,
    };
    if let Err(e) = mailrs_auth_guard::kevy::guard_for(Some(&url))
        .raise(alert)
        .await
    {
        tracing::error!(error = %e, "backup alert not raised");
    }
}

async fn audit(action: &'static str, detail: &str) {
    let Ok(url) = std::env::var("MAILRS_KEVY_URL") else {
        return;
    };
    let detail = detail.to_string();
    let res = tokio::task::spawn_blocking(move || {
        let mut c = kevy_client::Connection::connect(&url)?;
        mailrs_core_sidestate::families::audit::append(
            &mut c,
            "system",
            action,
            "backup",
            &detail,
            chrono::Utc::now().timestamp(),
        )
    })
    .await;
    if !matches!(res, Ok(Ok(()))) {
        tracing::error!(?res, "backup run not audited");
    }
}

fn hostname() -> String {
    std::env::var("MAILRS_HOSTNAME").unwrap_or_else(|_| "mailrs".into())
}

/// Bytes available to an unprivileged writer under `path`.
fn free_bytes(path: &Path) -> std::io::Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `st` a writable statvfs
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(st.f_bavail as u64 * st.f_frsize as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_name_a_kind_and_a_path() {
        let got = parse_sources(DEFAULT_SOURCES).unwrap();
        assert_eq!(got.len(), 4);
        assert_eq!(got[1].kind, SourceKind::Maildir);
        assert_eq!(got[2].path, PathBuf::from("/kevy-server"));
        assert!(parse_sources("x=disk:/a").is_err());
        assert!(parse_sources("x:/a").is_err());
    }
}
