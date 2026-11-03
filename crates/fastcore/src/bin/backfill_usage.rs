//! `mailrs-fastcore-backfill-usage` — recompute every user's mailbox
//! byte usage from maildir and SET the network-kevy counters the quota
//! stage reads (`mailrs:quota:<user>:used_bytes`).
//!
//! Walks `$MAILRS_MAILDIR/<domain>/<local>` (INBOX cur/new plus every
//! Maildir++ subfolder's cur/new) and sums file sizes. Idempotent —
//! SET overwrites whatever incremental drift the counters accumulated.
//! `GET /v1/admin/maintenance:usage-drift` on the running core reports the
//! drift this would undo, without writing.

fn main() {
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let url = std::env::var("MAILRS_KEVY_URL").expect("MAILRS_KEVY_URL required");
    let mut conn = kevy_client::Connection::connect(&url).expect("connect network kevy");
    let Ok(usage) = mailrs_fastcore::usage::maildir_usage(std::path::Path::new(&root)) else {
        eprintln!("maildir root {root} unreadable");
        std::process::exit(1);
    };
    for (user, bytes) in &usage {
        let key = mailrs_core_sidestate::keys::quota::used_key(user);
        conn.set(key.as_bytes(), bytes.to_string().as_bytes())
            .expect("kevy set");
        eprintln!("  user={user} used_bytes={bytes}");
    }
    eprintln!("done: {} users", usage.len());
}
