//! Shadows over the per-user **thread** projection.
//!
//! `.claude/rules/measure-before-you-cut-over.md` is the rule these serve:
//! the first reading is usually migration debt, not the defect.

use super::prelude::*;

/// `POST /v1/admin/maintenance:threadrow-shadow` — what the conversation
/// list would show if it were served from each user's own membership row
/// rather than the shared thread hash.
///
/// `mailrs:thread:{tid}` has no user segment, and eleven of its fields
/// describe one person's copy: the three counters, `starred`, `archived`,
/// `pinned`, `has_action`, `category`, the preview and the two importance
/// fields. Every local recipient's arrival lands on the same row, and
/// `hydrate_page` reads it — so on a thread two accounts both received,
/// what both of them see is whatever the last writer put there.
///
/// The membership row already carries all of it (RFC 20260730 S1 added the
/// display payload; the counters are maintained per user with `hincrby`).
/// This measures the disagreement before the read moves, per field, so the
/// cutover is a decision about a number rather than about an argument.
///
/// Read-only. Multi-owner threads are counted separately because they are
/// the only ones that *can* disagree — 74 of 30,586 on 2026-07-31, so a
/// difference concentrated there is the defect and a difference spread
/// across the rest is a backfill gap.
///
/// It was both. The first run reported 19,779 of 30,716 differing, on
/// `subject`, `senders_csv` and `importance_level` — membership rows that
/// predated the display payload, plus `set_thread_importance` writing only
/// the shared hash. `maintenance:backfill-thread-user` converged 19,778 of
/// them; what remained was 74 differences, 71 of them on the 74
/// multi-owner threads, all three per-user counters. That is the defect
/// and nothing else, which is what made the read safe to move.
///
/// Kept afterwards as a drift detector for the fields that still exist on
/// both sides. The counters are *expected* to differ on a multi-owner
/// thread — the shared hash sums every recipient's delivery — so a
/// difference there is only news when `differ` exceeds
/// `differ_multi_owner`.
pub(crate) async fn threadrow_shadow_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    // What the store did while this ran — see `store_motion`.
    let motion = crate::store_motion::begin(&state);
    use axum::response::IntoResponse;

    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Owners per thread, so "does this even have two readers" is answered
    // from data rather than assumed.
    let mut owners: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut per_user_threads: Vec<(String, Vec<String>)> = Vec::new();
    for user in &users {
        let tids = state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default();
        for tid in &tids {
            *owners.entry(tid.clone()).or_insert(0) += 1;
        }
        per_user_threads.push((user.clone(), tids));
    }

    let mut pairs_compared = 0u64;
    let mut row_missing = 0u64;
    let mut shared_missing = 0u64;
    let mut agree = 0u64;
    let mut differ = 0u64;
    let mut differ_multi_owner = 0u64;
    let mut by_field: std::collections::BTreeMap<&'static str, u64> =
        std::collections::BTreeMap::new();
    let mut samples: Vec<String> = Vec::new();

    for (user, tids) in &per_user_threads {
        for tid in tids {
            pairs_compared += 1;
            let mine = state
                .mailbox
                .get_thread_for_user(user, tid)
                .unwrap_or_default();
            let shared = state.mailbox.get_thread(tid).unwrap_or_default();
            let (mine, shared) = match (mine, shared) {
                (Some(m), Some(s)) => (m, s),
                // Counted apart because they mean opposite things: no
                // membership row is a thread the cutover would stop
                // showing, and no shared hash is one it would start
                // showing correctly.
                (None, _) => {
                    row_missing += 1;
                    continue;
                }
                (Some(_), None) => {
                    shared_missing += 1;
                    continue;
                }
            };
            let mut fields: Vec<&'static str> = Vec::new();
            if mine.subject != shared.subject {
                fields.push("subject");
            }
            if mine.senders_csv != shared.senders_csv {
                fields.push("senders_csv");
            }
            if mine.count != shared.count {
                fields.push("count");
            }
            if mine.unread_count != shared.unread_count {
                fields.push("unread_count");
            }
            if mine.sent_count != shared.sent_count {
                fields.push("sent_count");
            }
            if mine.latest_date != shared.latest_date {
                fields.push("latest_date");
            }
            if mine.latest_preview != shared.latest_preview {
                fields.push("latest_preview");
            }
            if mine.category != shared.category {
                fields.push("category");
            }
            // `starred` / `archived` / `pinned` / `has_action` are not
            // compared: the shared hash stopped carrying them when they
            // moved to the membership row, so every difference would be
            // one, and a check that always fires reports nothing.
            if mine.requires_action != shared.requires_action {
                fields.push("requires_action");
            }
            if mine.importance_level != shared.importance_level {
                fields.push("importance_level");
            }
            if fields.is_empty() {
                agree += 1;
                continue;
            }
            differ += 1;
            let multi = owners.get(tid).copied().unwrap_or(1) > 1;
            if multi {
                differ_multi_owner += 1;
            }
            for f in &fields {
                *by_field.entry(f).or_insert(0) += 1;
            }
            if samples.len() < 12 {
                samples.push(format!(
                    "{user} {tid} owners={} fields={}",
                    owners.get(tid).copied().unwrap_or(1),
                    fields.join(",")
                ));
            }
        }
    }

    Json(crate::store_motion::with_motion(
        serde_json::json!({
            "accounts": users.len(),
            "distinct_threads": owners.len(),
            "multi_owner_threads": owners.values().filter(|n| **n > 1).count(),
            // What it walked, so the three counts below are legible as a
            // fraction rather than as a bare number.
            "pairs_compared": pairs_compared,
            "agree": agree,
            "differ": differ,
            "differ_multi_owner": differ_multi_owner,
            "row_missing": row_missing,
            "shared_missing": shared_missing,
            "differ_by_field": by_field,
            "samples": samples,
        }),
        motion.finish(&state),
    ))
    .into_response()
}

/// `POST /v1/admin/maintenance:sent-axis-shadow?user=`
///
/// Compares the declared `is_sender` axis against the legacy
/// `user_threads_sent` zset, before the read path stops unioning them.
///
/// Both directions are reported separately because they mean opposite
/// things. `only_in_zset` is history the declared axis would lose if the
/// union were dropped now — it must be zero before that happens.
/// `only_in_axis` is the defect being fixed showing up as intended: the
/// zset has no ingest writer, so a send made since the last sweep is
/// expected to appear here.
///
/// Totals are reported alongside, so a pair of zeros cannot be confused
/// with having looked at nothing — the failure that made
/// `backfill-threading` answer `msgids_indexed: 9` and look fine.
pub(crate) async fn sent_axis_shadow_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    // What the store did while this ran — see `store_motion`.
    let motion = crate::store_motion::begin(&state);
    let users = match q.get("user") {
        Some(u) => vec![u.clone()],
        None => state.mailbox.list_account_addresses().unwrap_or_default(),
    };

    let mut report = Vec::new();
    for user in &users {
        let mut axis: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut offset = 0usize;
        loop {
            let page = state
                .mailbox
                .list_thread_ids_by_flag_via_table(user, "is_sender", 1000, offset, None)
                .unwrap_or_default();
            let short = page.len() < 1000;
            axis.extend(page);
            if short {
                break;
            }
            offset += 1000;
        }

        let zset: std::collections::HashSet<String> = state
            .mailbox
            .store_ref()
            .zrevrange(
                mailrs_mailbox_kevy::keys::user_threads_sent(user).as_bytes(),
                0,
                -1,
            )
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(b, _)| String::from_utf8(b).ok())
            .collect();

        let mut only_in_zset: Vec<&String> = zset.difference(&axis).collect();
        let mut only_in_axis: Vec<&String> = axis.difference(&zset).collect();
        only_in_zset.sort();
        only_in_axis.sort();

        // A thread id in the zset that no longer holds any message is not
        // history the axis is missing — it is a dead reference the zset kept
        // after a merge emptied the thread. Counting it as divergence would
        // block the cutover forever on entries whose loss costs nothing,
        // and eyeballing which is which is exactly the judgment call this
        // gate exists to remove.
        let live = |tid: &str| {
            !state
                .mailbox
                .list_thread_messages(user, tid)
                .unwrap_or_default()
                .is_empty()
        };
        let (only_in_zset_live, only_in_zset_dead): (Vec<&String>, Vec<&String>) =
            only_in_zset.iter().partition(|t| live(t.as_str()));

        if !only_in_zset_live.is_empty() {
            tracing::warn!(
                %user, count = only_in_zset_live.len(),
                "live threads the legacy sent zset holds and the declared axis does not — \
                 dropping the union now would lose them"
            );
        }

        report.push(serde_json::json!({
            "user": user,
            "axis_threads": axis.len(),
            "zset_threads": zset.len(),
            // The gate: threads that still hold messages and are absent from
            // the axis. Must be zero before the union is dropped.
            "only_in_zset_live": only_in_zset_live.len(),
            // Dead references the zset kept. Not a blocker; they disappear
            // with the zset.
            "only_in_zset_dead": only_in_zset_dead.len(),
            "only_in_axis": only_in_axis.len(),
            "only_in_zset_live_samples": only_in_zset_live.iter().take(8).collect::<Vec<_>>(),
            "only_in_zset_dead_samples": only_in_zset_dead.iter().take(8).collect::<Vec<_>>(),
            "only_in_axis_samples": only_in_axis.iter().take(8).collect::<Vec<_>>(),
        }));
    }

    Json(crate::store_motion::with_motion(
        serde_json::json!({ "users_checked": users.len(), "report": report }),
        motion.finish(&state),
    ))
    .into_response()
}
