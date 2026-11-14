//! The review screen's own routes: what was held, and letting one go.
//!
//! Held mail is neither Junk nor deleted. It stays in the maildir
//! because it is the evidence the abuse reports are built from — the
//! ones sent on 2026-08-27 attached the originals — and it stays out
//! of every ordinary list because a reader should not run into an
//! attempt to defraud them by accident.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use crate::*;

/// What a caller may narrow the review list by.
///
/// Paged like every other list, and for a reason that is not
/// hypothetical: a route that ignored the cursor would answer the
/// second page with the first one's rows, and the client — which
/// appends pages — would show every conversation twice.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct QuarantineQuery {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    before_ts: Option<i64>,
    /// The cursor of the last row of the previous page.
    #[serde(default)]
    cursor: Option<String>,
}

/// `GET /v1/users/{user}/quarantine` — newest first.
///
/// Reads the held range directly rather than filtering a live page:
/// the count and the rows come out of the same walk, so a page's size
/// cannot disagree with what is on it.
pub(crate) async fn list_quarantined(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    axum::extract::Query(q): axum::extract::Query<QuarantineQuery>,
) -> Result<Json<mailrs_core_api::method::thread::QuarantineListResponse>, axum::http::StatusCode> {
    use mailrs_core_api::paging::Paging;
    // 1000, not 200. The old ceiling was below the number actually
    // held — 439 on 2026-08-31 — and a caller who asked for more got
    // a short page with nothing to say it was short. The bound stays
    // because a route that will hydrate however many rows it is asked
    // for is a way to make the process do arbitrary work; it is now
    // far enough above the real figure to be a guard rather than a
    // silent truncation, and `total` says when it bites.
    // `min`, not `clamp(1, …)`. Zero asks only for the count, and
    // the count is an index read the walk does anyway — so the review
    // tab can put a number beside its name without hydrating rows it
    // is not going to show.
    let limit = q.limit.unwrap_or(50).min(1000);
    let paged = mailrs_core_api::types::ConversationFilter {
        before_ts: q.before_ts,
        after_cursor: q.cursor,
        ..Default::default()
    };
    let fetch = |before_ts: Option<i64>, n: usize| {
        let filter = ListThreadsFilter {
            quarantine: QuarantineScope::Only,
            before_ts,
            ..Default::default()
        };
        let (rows, total) = state
            .mailbox
            .list_threads_by_activity(&user, &filter, 0, n)
            .unwrap_or_else(|e| {
                // An error here reads as "nothing was held", which is the
                // one answer a reader cannot tell from the true one.
                tracing::warn!(%user, error = %e, "quarantine list failed; serving empty");
                (Vec::new(), 0)
            });
        let items: Vec<_> = rows
            .into_iter()
            .map(crate::routes::message_ops::row_to_wire)
            .collect();
        (items, total)
    };
    let (mut items, total) = match paged.paging(limit) {
        Err(_) => return Err(axum::http::StatusCode::BAD_REQUEST),
        Ok(Paging::BeforeTs(ts)) => fetch(Some(ts), limit),
        Ok(Paging::Cursor(mut pager)) => loop {
            let ask = pager.ask();
            let (rows, total) = fetch(ask.before_ts, ask.limit);
            if let Some(page) = pager.offer(rows, |r| r.last_date, |r| r.thread_id.as_str()) {
                break (page, total);
            }
        },
    };
    paged.stamp_cursors(&mut items);
    let items = crate::routes::labels::with_labels(&state, &user, items);
    let items = crate::sections::with_sections(&state, &user, items);
    Ok(Json(
        mailrs_core_api::method::thread::QuarantineListResponse { items, total },
    ))
}

/// `GET /v1/users/{user}/messages/{message_id}/fraud-verdict` — what
/// was decided about this message, when it arrived.
///
/// Served as the stored bytes rather than a re-derivation: the screen
/// has to show the rules that were in force then, and the verdict
/// carries its own version stamp for exactly that reason.
///
/// A message nobody suspected has no verdict, and the answer is
/// `null` rather than an empty one — "nothing was found" and "it was
/// examined and cleared" are different facts.
///
/// The verdict is stored by Message-ID for every recipient at once, so
/// the caller must have the message: another account's is a 404. Where
/// the caller's own copy was filed, and why, is stored per recipient and
/// joined to it here.
pub(crate) async fn get_fraud_verdict(
    State(state): State<Arc<FastcoreState>>,
    Path((user, message_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    match state.mailbox.user_message_facts(&user, &message_id) {
        Ok(Some(_)) => {}
        Ok(None) => return Err(axum::http::StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::warn!(%message_id, error = %e, "reading the message's owner failed");
            return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        }
    }
    let stored = state
        .mailbox
        .fraud_verdict(&message_id)
        .unwrap_or_else(|e| {
            tracing::warn!(%message_id, error = %e, "reading the verdict failed");
            None
        });
    let verdict = stored
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok());
    let placement = state
        .mailbox
        .message_placement(&user, &message_id)
        .unwrap_or_else(|e| {
            tracing::warn!(%message_id, error = %e, "reading the placement failed");
            None
        })
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    let verdict = mailrs_intake::why::joined(verdict, placement);
    Ok(Json(serde_json::json!({ "verdict": verdict })))
}
