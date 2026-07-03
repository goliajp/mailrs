//! `mailrs-fastcore-backfill-usage` — recompute every user's mailbox
//! byte usage from maildir and SET the network-kevy counters the quota
//! stage reads (`mailrs:quota:<user>:used_bytes`).
//!
//! Walks `$MAILRS_MAILDIR/<domain>/<local>` (INBOX cur/new plus every
//! Maildir++ subfolder's cur/new) and sums file sizes. Idempotent —
//! SET overwrites whatever incremental drift the counters accumulated.
//! Run any time; no locks needed (counters are advisory, enforcement
//! is fail-open).

use std::path::Path;

fn dir_bytes(dir: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(iter) = std::fs::read_dir(dir) {
        for e in iter.flatten() {
            if let Ok(md) = e.metadata()
                && md.is_file()
            {
                total += md.len();
            }
        }
    }
    total
}

fn user_bytes(base: &Path) -> u64 {
    let mut total = dir_bytes(&base.join("cur")) + dir_bytes(&base.join("new"));
    if let Ok(iter) = std::fs::read_dir(base) {
        for e in iter.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with('.') || name == "." || name == ".." {
                continue;
            }
            let sub = e.path();
            if sub.join("new").is_dir() {
                total += dir_bytes(&sub.join("cur")) + dir_bytes(&sub.join("new"));
            }
        }
    }
    total
}

fn main() {
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let url = std::env::var("MAILRS_KEVY_URL").expect("MAILRS_KEVY_URL required");
    let mut conn = kevy_client::Connection::open(&url).expect("connect network kevy");

    let mut users = 0u64;
    let Ok(domains) = std::fs::read_dir(&root) else {
        eprintln!("maildir root {root} unreadable");
        std::process::exit(1);
    };
    for d in domains.flatten() {
        if !d.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let domain = d.file_name().to_string_lossy().into_owned();
        let Ok(locals) = std::fs::read_dir(d.path()) else {
            continue;
        };
        for l in locals.flatten() {
            if !l.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let local = l.file_name().to_string_lossy().into_owned();
            let user = format!("{local}@{domain}").to_lowercase();
            let bytes = user_bytes(&l.path());
            let key = format!("mailrs:quota:{user}:used_bytes");
            conn.set(key.as_bytes(), bytes.to_string().as_bytes())
                .expect("kevy set");
            eprintln!("  user={user} used_bytes={bytes}");
            users += 1;
        }
    }
    eprintln!("done: {users} users");
}
