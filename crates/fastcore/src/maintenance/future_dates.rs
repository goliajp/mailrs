//! Stored mail dated later than it arrived.
//!
//! Nothing can arrive before it was written, so a `Date:` later than
//! the arrival is not a time at all, and the message sorts at its
//! arrival instead (`headers::sort_date`). Ingest applies that to new
//! mail; this applies it to mail stored before it did. The maildir
//! self-heal cannot: it only ever raises a date, and every one of these
//! is too high.
//!
//! A stored date later than the arrival is wrong whatever wrote it, and
//! production has rows whose date matches neither the file's `Date:`
//! nor its arrival. So the date is recomputed from the file the way
//! ingest computes it, not copied from the arrival; a copy whose file
//! is gone falls back to the arrival, which is still a time the
//! message existed.
//!
//! Arrival is the delivery second in the maildir filename, the same
//! source ingest uses. Read-only by default; `{"repair": true}` rewrites
//! the message and then lowers the conversation's row to its newest
//! inbound message, in the shared hash and the membership row together.

use super::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

use super::recompute_sender_trust::folder_of;
use crate::headers::{maildir_filename_epoch, sort_date};

#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct Request {
    #[serde(default)]
    pub repair: bool,
}

#[derive(Debug, Default, serde::Serialize)]
pub(crate) struct Report {
    /// Copies looked at, so "0 found" is told apart from "0 read".
    pub walked: u64,
    /// Copies whose file name carries no delivery time to compare with.
    pub no_arrival: u64,
    /// Copies dated later than they arrived.
    pub dated_ahead: u64,
    /// Of those, how many had no file to read a `Date:` from and take
    /// their arrival.
    pub no_file: u64,
    /// Of those, how many were rewritten (0 unless `repair`).
    pub repaired: u64,
    /// Conversation rows lowered after their messages were.
    pub rows_lowered: u64,
    pub errors: u64,
    pub samples: Vec<String>,
}

pub(crate) async fn future_dates_route(
    State(state): State<Arc<FastcoreState>>,
    body: Option<Json<Request>>,
) -> axum::response::Response {
    let req = body.map(|Json(r)| r).unwrap_or_default();
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut report = Report::default();
    for user in &users {
        let files = files_of(&state, user);
        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            let mut touched = false;
            for mid in state
                .mailbox
                .user_thread_message_ids(user, &tid)
                .unwrap_or_default()
            {
                if repair_message(&state, &files, user, &tid, &mid, req.repair, &mut report) {
                    touched = true;
                }
            }
            if touched && lower_row(&state, user, &tid) {
                report.rows_lowered += 1;
            }
        }
    }

    tracing::info!(
        walked = report.walked,
        ahead = report.dated_ahead,
        repaired = report.repaired,
        rows = report.rows_lowered,
        "future-dated mail"
    );
    Json(report).into_response()
}

// whether the copy was rewritten
fn repair_message(
    state: &FastcoreState,
    files: &HashMap<(String, String), PathBuf>,
    user: &str,
    tid: &str,
    mid: &str,
    repair: bool,
    report: &mut Report,
) -> bool {
    let Ok(Some(bytes)) = state.mailbox.user_message_view(user, mid) else {
        return false;
    };
    let Ok(mut wire) =
        serde_json::from_slice::<mailrs_core_api::method::message::MessageWire>(&bytes)
    else {
        report.errors += 1;
        return false;
    };
    report.walked += 1;
    let (folder, leaf) = match wire.blob_ref.rsplit_once('/') {
        Some((f, l)) if f.starts_with('.') => (f.to_string(), l),
        _ => (String::new(), wire.blob_ref.as_str()),
    };
    let base = leaf.split(':').next().unwrap_or(leaf).to_string();
    let Some(arrival) = maildir_filename_epoch(&base) else {
        report.no_arrival += 1;
        return false;
    };
    if wire.internal_date <= arrival {
        return false;
    }
    report.dated_ahead += 1;
    let header = match files.get(&(folder, base)).map(std::fs::read) {
        Some(Ok(raw)) => mailrs_inbound::dates::date_header_epoch(&raw).unwrap_or(0),
        _ => {
            report.no_file += 1;
            0
        }
    };
    let date = sort_date(header, Some(arrival)).unwrap_or(arrival);
    if report.samples.len() < 20 {
        report.samples.push(format!(
            "{user} {mid} stored={} arrival={arrival} header={header} now={date}",
            wire.internal_date
        ));
    }
    if !repair {
        return false;
    }
    wire.internal_date = date;
    wire.date = date;
    let Ok(payload) = serde_json::to_vec(&wire) else {
        report.errors += 1;
        return false;
    };
    // the write keeps a stored value wherever it is handed a blank or a
    // zero, so this user's facts are passed back as read
    let Ok(Some(facts)) = state.mailbox.user_message_facts(user, mid) else {
        report.errors += 1;
        return false;
    };
    let written = state.mailbox.upsert_user_message(
        user,
        tid,
        mid,
        date,
        &payload,
        &mailrs_mailbox_kevy::UserMessageFacts {
            blob_ref: &facts.blob_ref,
            uid: facts.uid,
            flags: facts.flags,
            modseq: facts.modseq,
        },
    );
    if let Err(e) = written {
        tracing::warn!(err = %e, %user, %mid, "future-dates: write failed");
        report.errors += 1;
        return false;
    }
    report.repaired += 1;
    true
}

// the row follows the newest inbound message; a thread whose newest
// message is the user's own keeps its date, as `thread-date-audit` does
fn lower_row(state: &FastcoreState, user: &str, tid: &str) -> bool {
    let Ok(Some(row)) = state.mailbox.get_thread(tid) else {
        return false;
    };
    let Ok(wires) = state.mailbox.thread_messages_unscoped(tid) else {
        return false;
    };
    let Some(display) = mailrs_mailbox_kevy::display_message(&wires, user) else {
        return false;
    };
    let sender = display["sender"].as_str().unwrap_or("");
    if mailrs_mailbox_kevy::senders_csv_contains_user(sender, user) {
        return false;
    }
    let want = display["internal_date"].as_i64().unwrap_or(0);
    if want <= 0 || row.latest_date <= want {
        return false;
    }
    state
        .mailbox
        .set_thread_display_date(user, tid, want)
        .unwrap_or(false)
}

// every file in this user's maildirs, keyed the way a `blob_ref` names
// it: folder plus the name before the `:2,` flags, which a rename keeps
fn files_of(state: &Arc<FastcoreState>, user: &str) -> HashMap<(String, String), PathBuf> {
    let mut out = HashMap::new();
    for mb in crate::imap::backend::list_mailboxes(state, user) {
        let folder = folder_of(std::path::Path::new(&mb.path));
        for sub in ["new", "cur"] {
            let Ok(rd) = std::fs::read_dir(std::path::Path::new(&mb.path).join(sub)) else {
                continue;
            };
            for e in rd.flatten() {
                let path = e.path();
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let base = name.split(':').next().unwrap_or(name).to_string();
                out.insert((folder.clone(), base), path);
            }
        }
    }
    out
}
