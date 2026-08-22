//! The POP3 arm of the connected-mailbox sync.
//!
//! Split from `external_sync.rs` when the three protocols together
//! passed the 500-line limit. They divide cleanly, because the only
//! thing they share is where they end — `ingest_delivered_file`, the
//! same door the spool uses — and what each remembers between syncs is
//! the whole of their difference.

use std::sync::Arc;

use super::external_sync::{now_secs, sanitise};
use crate::FastcoreState;
use mailrs_core_sidestate::families::external_accounts::{self as ext, AccountRow};
use mailrs_pop3_client as pop3;

/// Read one POP3 account.
///
/// POP3 has no folders and no server-side state worth reading, so this
/// is the whole of it: ask what is there, work out what is new, fetch
/// those.
///
/// **Nothing is deleted.** POP3's `DELE` is how most clients keep a
/// mailbox small, and it is also how somebody loses mail they thought
/// was safe — the message is gone from the server the moment `QUIT`
/// commits it, and this is a second reader of that mailbox, not its
/// owner. Leaving it is the choice that can be undone.
pub(crate) async fn sync_pop3(
    state: &Arc<FastcoreState>,
    user: &str,
    row: &AccountRow,
    secret: &str,
) -> Result<usize, String> {
    let tls = match row.incoming.tls {
        ext::Tls::Implicit => pop3::Tls::Implicit,
        ext::Tls::StartTls => pop3::Tls::StartTls,
        ext::Tls::None => pop3::Tls::None,
    };
    let mut session = pop3::Session::connect(&row.incoming.host, row.incoming.port, tls)
        .await
        .map_err(|e| e.to_string())?;
    let login = row.username.clone().unwrap_or_else(|| row.email.clone());
    session
        .login(&login, secret)
        .await
        .map_err(|e| e.to_string())?;

    // The uids already held, from the same marker the IMAP path uses —
    // one field, one list, because a uid the reader forgot is a message
    // downloaded twice and a uid it invented is a message never seen.
    let held = read_pop3_marker(state, &row.id);
    let on_server = session.uidl().await.map_err(|e| e.to_string())?;
    let want = pop3::not_yet_held(&on_server, &held);

    let mut filed = 0usize;
    let mut now_held = held.clone();
    for msg in want {
        let body = session.retr(msg.number).await.map_err(|e| e.to_string())?;
        let blob = format!("ext-{}-pop3-{}", row.id, sanitise(&msg.uid));
        crate::ingest::ingest_delivered_file(state, user, &blob, &body, "INBOX");
        now_held.push(msg.uid.clone());
        filed += 1;
    }
    let _ = session.quit().await;
    write_pop3_marker(state, &row.id, &now_held);
    Ok(filed)
}

/// The uids already held for a POP3 account.
///
/// A list rather than a high-water mark, because POP3 has no monotonic
/// sequence: message numbers are per-session and renumber whenever
/// anything is deleted. A uid the reader forgets is a message
/// downloaded twice; a uid it invents is a message never seen.
///
/// Stored as one newline-joined field. A uid may contain any printable
/// character **except** a newline (RFC 1939 §7 — one uid per line), so
/// that is the one separator the value cannot contain.
fn read_pop3_marker(state: &Arc<FastcoreState>, account_id: &str) -> Vec<String> {
    let Some(mut conn) = state.net_conn() else {
        return Vec::new();
    };
    let key = format!("ext:sync:{account_id}:pop3");
    let Ok(Some(raw)) = conn.hget(key.as_bytes(), b"uids") else {
        return Vec::new();
    };
    String::from_utf8_lossy(&raw)
        .split('\n')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// How many uids one account remembers.
///
/// Bounded, because the list only grows: a mailbox somebody never
/// prunes would otherwise turn one hash field into a megabyte, and the
/// oldest uids are the ones least likely to still be on the server.
/// Keeping the newest is what stops a re-download.
const POP3_UIDS_REMEMBERED: usize = 5_000;

fn write_pop3_marker(state: &Arc<FastcoreState>, account_id: &str, uids: &[String]) {
    let Some(mut conn) = state.net_conn() else {
        return;
    };
    let start = uids.len().saturating_sub(POP3_UIDS_REMEMBERED);
    let joined = uids[start..].join("\n");
    let at = now_secs().to_string();
    let key = format!("ext:sync:{account_id}:pop3");
    let _ = conn.hset(
        key.as_bytes(),
        &[
            (b"uids".as_slice(), joined.as_bytes()),
            (b"at".as_slice(), at.as_bytes()),
        ],
    );
}
