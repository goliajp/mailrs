//! A spool file the drain died on [`super::attempts::LIMIT`] times is set
//! aside in `<spool>/quarantine/`, so the next message is not held behind
//! it and the process stops restarting on it.
//!
//! Beside it, `<F>.json` says what is known without reading the message:
//! the envelope, its size, the attempts, the step the last one started and
//! the steps already done. The postmaster gets a notice built from the same
//! envelope facts only, never from the message: a notice that parsed the
//! message could take the process down the same way.
//!
//! To try again once a fixed build is running, move the file back on the
//! host: `mv <spool>/quarantine/F <spool>/incoming/new/F`. It keeps its
//! name, so the steps already done are skipped and nobody gets it twice.

use std::path::{Path, PathBuf};

use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
use serde::{Deserialize, Serialize};

use super::attempts::{self, Record};

pub(crate) fn dir(spool_root: &Path) -> PathBuf {
    spool_root.join("quarantine")
}

/// Where the record of quarantined file `filename` is kept.
pub(crate) fn record_path(spool_root: &Path, filename: &str) -> PathBuf {
    dir(spool_root).join(format!("{filename}.json"))
}

/// What `<F>.json` holds.
#[derive(Debug, Serialize, Deserialize)]
pub struct Quarantined {
    pub file: String,
    pub quarantined_at: i64,
    pub reverse_path: String,
    pub forward_paths: Vec<String>,
    pub size: u64,
    pub record: Record,
}

/// The record kept beside quarantined file `filename`, if any.
pub(crate) fn record_beside(spool_root: &Path, filename: &str) -> Option<Record> {
    let raw = std::fs::read(record_path(spool_root, filename)).ok()?;
    serde_json::from_slice::<Quarantined>(&raw)
        .ok()
        .map(|q| q.record)
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Move spool file `path` into quarantine with its record, and tell the
/// postmaster at `primary_domain`. True when the file was moved.
pub(crate) fn set_aside(
    spool_root: &Path,
    path: &Path,
    filename: &str,
    env: &SpoolEnvelope,
    size: u64,
    record: Record,
    primary_domain: Option<&str>,
) -> bool {
    let q = Quarantined {
        file: filename.to_string(),
        quarantined_at: now(),
        reverse_path: env.reverse_path.clone(),
        forward_paths: env.forward_paths.clone(),
        size,
        record,
    };
    // the record first: a file in quarantine without one would come back
    // with its done steps forgotten
    let moved = attempts::write_atomic(&record_path(spool_root, filename), &q)
        .and_then(|()| std::fs::rename(path, dir(spool_root).join(filename)));
    if let Err(e) = moved {
        tracing::error!(file = %filename, error = %e, "spool file not quarantined; it will be tried again");
        return false;
    }
    let attempt = attempts::dir(spool_root).join(filename);
    if let Err(e) = std::fs::remove_file(&attempt) {
        tracing::warn!(path = %attempt.display(), error = %e, "spool attempt record not removed");
    }
    tracing::error!(
        event = "spool_quarantined",
        file = %filename,
        attempts = q.record.attempts,
        stage = %q.record.stage,
        done = ?q.record.done,
        from = %q.reverse_path,
        to = ?q.forward_paths,
        size,
        "spool file stopped the process repeatedly; set aside"
    );
    if let Some(domain) = primary_domain {
        notify(spool_root, domain, &q);
    }
    true
}

/// Drop line breaks and other control characters from a value an SMTP
/// client chose, before it goes into the notice's text.
fn plain(s: &str) -> String {
    s.chars()
        .map(|c| match c.is_control() {
            true => ' ',
            false => c,
        })
        .collect()
}

/// The notice, as a message of our own composing.
pub(crate) fn notice_body(domain: &str, q: &Quarantined) -> Vec<u8> {
    let date = chrono::Utc::now().to_rfc2822();
    let mut text = String::new();
    text.push_str(&format!("From: Mail system <postmaster@{domain}>\r\n"));
    text.push_str(&format!("To: postmaster@{domain}\r\n"));
    text.push_str("Subject: A received message was set aside\r\n");
    text.push_str(&format!("Date: {date}\r\n"));
    text.push_str(&format!(
        "Message-ID: <quarantine.{}.{}@{domain}>\r\n",
        q.quarantined_at,
        plain(&q.file).replace([' ', '<', '>', '@'], "_")
    ));
    text.push_str("Content-Type: text/plain; charset=utf-8\r\n\r\n");
    text.push_str(
        "The mail server stopped while delivering this message, each time it\r\n\
         tried, and has set it aside so other mail keeps arriving. It was not\r\n\
         delivered to the recipients below that are not listed as done.\r\n\r\n",
    );
    text.push_str(&format!("Spool file: {}\r\n", plain(&q.file)));
    text.push_str(&format!("MAIL FROM: {}\r\n", plain(&q.reverse_path)));
    for rcpt in &q.forward_paths {
        text.push_str(&format!("RCPT TO: {}\r\n", plain(rcpt)));
    }
    text.push_str(&format!("Size: {} bytes\r\n", q.size));
    text.push_str(&format!("Attempts: {}\r\n", q.record.attempts));
    text.push_str(&format!("Last step: {}\r\n", plain(&q.record.stage)));
    for step in &q.record.done {
        text.push_str(&format!("Done: {}\r\n", plain(step)));
    }
    text.push_str(
        "\r\nOnce a fixed build runs, move the file from quarantine/ back to\r\n\
         incoming/new/ under the spool to deliver it.\r\n",
    );
    text.into_bytes()
}

/// Spool the notice for the postmaster, so it is routed like any mail to
/// that address (an alias, or the role fallback) rather than only to an
/// account of that name.
fn notify(spool_root: &Path, domain: &str, q: &Quarantined) {
    let env = SpoolEnvelope {
        reverse_path: String::new(),
        forward_paths: vec![format!("postmaster@{domain}")],
        is_authenticated: false,
        auth_user: None,
        conn_id: 0,
        target_folder: "INBOX".into(),
        received_at: q.quarantined_at as u64,
        fraud_verdict: None,
        sandbox_ns: None,
        schema_version: SPOOL_SCHEMA_VERSION,
    };
    let blob = encode_spool_blob(&env, &notice_body(domain, q));
    let incoming = spool_root.join("incoming");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let name = format!(
        "{}.M{nanos}P{}.quarantine-notice",
        q.quarantined_at,
        std::process::id()
    );
    let tmp = incoming.join("tmp").join(&name);
    let written = std::fs::create_dir_all(incoming.join("tmp"))
        .and_then(|()| std::fs::create_dir_all(incoming.join("new")))
        .and_then(|()| std::fs::write(&tmp, &blob))
        .and_then(|()| std::fs::rename(&tmp, incoming.join("new").join(&name)));
    match written {
        Ok(()) => {
            tracing::info!(file = %q.file, notice = %name, "postmaster told of a quarantined spool file")
        }
        Err(e) => tracing::warn!(file = %q.file, error = %e, "quarantine notice not spooled"),
    }
}

/// Files in quarantine, not counting their records.
pub(crate) fn count(spool_root: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir(spool_root)) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            !name.ends_with(".json") && !name.ends_with(".tmp")
        })
        .count() as u64
}
