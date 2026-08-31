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
) -> Json<mailrs_core_api::method::thread::QuarantineListResponse> {
    let filter = ListThreadsFilter {
        quarantine: QuarantineScope::Only,
        before_ts: q.before_ts,
        ..Default::default()
    };
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
    let (rows, total) = state
        .mailbox
        .list_threads_by_activity(&user, &filter, 0, limit)
        .unwrap_or_else(|e| {
            // An error here reads as "nothing was held", which is the
            // one answer a reader cannot tell from the true one.
            tracing::warn!(%user, error = %e, "quarantine list failed; serving empty");
            (Vec::new(), 0)
        });
    let items = rows
        .into_iter()
        .map(crate::routes::message_ops::row_to_wire)
        .collect();
    Json(mailrs_core_api::method::thread::QuarantineListResponse { items, total })
}

/// `POST /v1/users/{user}/quarantine/{thread_id}/release` — it was not
/// fraud.
///
/// Returns the conversation to whatever list it belonged to. The
/// verdict on the message is left alone: it records what was decided
/// then, and a person disagreeing later does not change what the
/// rules saw.
pub(crate) async fn release_quarantined(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = state
        .mailbox
        .set_quarantined(&user, &thread_id, false)
        .unwrap_or(false);
    if ok {
        // Logged rather than merely done: a false positive is the one
        // failure mode this feature can have, and a release nobody can
        // count is a failure rate nobody can measure.
        tracing::info!(%user, %thread_id, "released: a person said this was not fraud");
    }
    crate::routes::message_ops::action_result(ok)
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
pub(crate) async fn get_fraud_verdict(
    State(state): State<Arc<FastcoreState>>,
    Path((_user, message_id)): Path<(String, String)>,
) -> Json<serde_json::Value> {
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
    Json(serde_json::json!({ "verdict": verdict }))
}
