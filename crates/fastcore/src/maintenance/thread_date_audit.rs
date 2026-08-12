//! Threads whose row is dated later than anything that arrived in them.
//!
//! The row follows the last **inbound** message. Two paths did not know
//! that — the rethread merge took whichever side was fresher, and the
//! split took the last message outright — so replying with a changed
//! subject stamped the conversation with the reply's time and moved it
//! to the top of Inbox. Both are fixed; this answers what they left
//! behind, because the maildir self-heal cannot: it only ever raises
//! `latest_date`, and every one of these is too *high*.
//!
//! Read-only by default. `{"repair": true}` writes the corrected date
//! to the shared hash and the membership row together — they carry the
//! same fact under two names (`latest_date` / `activity`) and every
//! axis sorts on the row's copy.

use crate::FastcoreState;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Request {
    #[serde(default)]
    pub repair: bool,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct Report {
    /// Threads walked. Reported so "0 wrong" is distinguishable from
    /// "0 looked at" — a number that cannot come out zero is not a
    /// verification.
    pub examined: u64,
    /// Rows dated later than their newest inbound message.
    pub ahead_of_inbound: u64,
    /// Of those, how many were corrected (0 unless `repair`).
    pub repaired: u64,
    /// Threads with no inbound message at all: a sent-only thread's own
    /// send is the only date there is, and it stays.
    pub sent_only: u64,
    /// Threads whose messages could not be read, so nothing is claimed.
    pub unreadable: u64,
    /// A few examples, worth more than the count when deciding to repair.
    pub samples: Vec<String>,
}

pub(crate) async fn thread_date_audit_route(
    State(state): State<Arc<FastcoreState>>,
    body: Option<axum::Json<Request>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let req = body.map(|axum::Json(r)| r).unwrap_or_default();

    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut report = Report::default();
    for user in &users {
        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            report.examined += 1;
            let Ok(row) = state.mailbox.get_thread(&tid) else {
                report.unreadable += 1;
                continue;
            };
            let Some(row) = row else {
                report.unreadable += 1;
                continue;
            };
            let Ok(wires) = state.mailbox.thread_messages_unscoped(&tid) else {
                report.unreadable += 1;
                continue;
            };
            let Some(display) = mailrs_mailbox_kevy::display_message(&wires, user) else {
                report.unreadable += 1;
                continue;
            };
            let sender = display["sender"].as_str().unwrap_or("");
            if mailrs_mailbox_kevy::senders_csv_contains_user(sender, user) {
                // The newest thing here is the user's own, so there is
                // no inbound message to be ahead of.
                report.sent_only += 1;
                continue;
            }
            let want = display["internal_date"].as_i64().unwrap_or(0);
            if want <= 0 || row.latest_date <= want {
                continue;
            }
            report.ahead_of_inbound += 1;
            if report.samples.len() < 8 {
                report.samples.push(format!(
                    "{user} {tid} row={} inbound={want} ahead_by={}s",
                    row.latest_date,
                    row.latest_date - want
                ));
            }
            if req.repair
                && state
                    .mailbox
                    .set_thread_display_date(user, &tid, want)
                    .is_ok()
            {
                report.repaired += 1;
            }
        }
    }

    tracing::info!(
        examined = report.examined,
        ahead = report.ahead_of_inbound,
        repaired = report.repaired,
        "thread date audit"
    );
    axum::Json(report).into_response()
}
