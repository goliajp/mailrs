//! Two questions about the declared access paths that nothing could answer
//! before kevy 5.1, and that a table-driven store has to be able to answer.
//!
//! **Is a query running without a declared path to serve it?** The engine
//! refuses those rather than scanning, so the refusal is observable — it just
//! had nowhere to be read from. `idx_advise` returns the refusal families
//! most-refused first, each rendered as the declaration that would have served
//! it, so the answer arrives as the command to run.
//!
//! **Is a declared path carrying no queries?** Only partly answerable, and
//! this route reports the engine's facts rather than deriving a verdict it
//! cannot stand behind.
//!
//! `idx_usage` gives `(hits, last_hit, declared_at)`, but the engine records a
//! hit on exactly four call sites — the two claused queries and the two cold
//! ones. **Plain `idx_query` records nothing**, and that is how this crate
//! reads every ORDERPATH: `table_query` computes the composite byte bounds
//! itself and scans. So the flag axes (read through `idx_query_claused`)
//! accumulate hits and the ORDERPATHs never will.
//!
//! Measured on a fresh store after real traffic (2026-08-13): **all 18
//! declared paths report no usage cell at all** — `idx_usage` returns `None`
//! for every one, including the standalone `idx_create` indexes. So the table
//! is not merely missing the ORDERPATHs; it appears never to be populated
//! here. Why has not been isolated, and is written up for upstream in
//! `.claude/notes/kevy-5.1-dogfood-feedback-2026-08-13.md` §8b rather than
//! guessed at here.
//!
//! The first version of this route collapsed "no cell" into "zero hits" and so
//! reported every path as unused, immediately after `threaduser.by_user_bucket`
//! had served five pages. A verdict that cannot come out right is not a
//! verdict. `has_usage_cell` and the raw counters are reported instead, and
//! turning "no hits" into "unused" is left to someone who knows which paths
//! are read through which API.
//!
//! The names come from the engine (`idx_list`, `table_list`), never from a
//! list in this file. A hand-kept list of index names is the shape that rots:
//! `check-inert-fields.sh` and the legacy-zset sweep both exist because
//! something here fell out of step with what the store actually held.
//!
//! Read-only. It reports and returns; changing a declaration stays a human
//! act, which is also why `autodeclare` is 0 on our table.

use super::prelude::*;

/// `POST /v1/admin/maintenance:idx-advice`
///
/// ```json
/// {
///   "unserved": [{"count": 412, "asked_for": "threaduser.by_user_snoozed",
///                 "advice": "TABLE.ALTER threaduser ORDERPATH …"}],
///   "declared": [{"name": "threaduser.by_user_bucket", "kind": "Range",
///                 "hits": 91021, "last_hit_s": 1755000000,
///                 "declared_s": 1754000000, "unused": false}],
///   "unused_count": 0
/// }
/// ```
///
/// `unserved` empty is the healthy shape, and it is the half that works: the
/// engine refuses an undeclared query rather than scanning, so a family here
/// is a real query with no path. It is still not a failure on its own — a
/// refusal family can be one malformed query rather than a missing
/// declaration, which is why the advice string is reported and not applied.
///
/// The `declared` rows carry counters, not judgements. See the module header
/// for why `hits: 0` does not mean unused here.
pub(crate) async fn idx_advice_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let store = state.mailbox.store_ref();

    // Queries the engine refused because no declared path covered them. The
    // log clears on every catalog mutation, so this is "since the last
    // declaration change", not "ever".
    let unserved: Vec<_> = store
        .idx_advise()
        .into_iter()
        .map(|a| {
            serde_json::json!({
                "count": a.count,
                "asked_for": String::from_utf8_lossy(&a.name),
                "advice": a.advice,
            })
        })
        .collect();

    // Every declared path comes from `idx_list` alone — a declared table
    // registers its orderpaths and per-column indexes there too, so walking
    // `table_list` as well listed each of them twice under two different kind
    // labels. The tables are read only to say which names are orderpaths,
    // because that is the set whose hits will always read zero.
    let orderpaths: std::collections::HashSet<String> = store
        .table_list()
        .iter()
        .flat_map(|spec| {
            let table = String::from_utf8_lossy(&spec.name).into_owned();
            spec.orderpaths
                .iter()
                .map(move |p| format!("{table}.{}", String::from_utf8_lossy(&p.name)))
                .collect::<Vec<_>>()
        })
        .collect();
    let mut names: Vec<(String, String)> = store
        .idx_list()
        .into_iter()
        .map(|(name, _prefix, kind)| {
            (
                String::from_utf8_lossy(&name).into_owned(),
                format!("{kind:?}"),
            )
        })
        .collect();
    names.sort();
    names.dedup();

    let mut no_cell = 0usize;
    let declared: Vec<_> = names
        .into_iter()
        .map(|(name, kind)| {
            let usage = store.idx_usage(name.as_bytes());
            if usage.is_none() {
                no_cell += 1;
            }
            let is_orderpath = orderpaths.contains(&name);
            let (hits, last, declared_s) = usage.unwrap_or((0, 0, 0));
            serde_json::json!({
                "name": name,
                "kind": kind,
                // Absent and zero are different states and were previously
                // indistinguishable here, which is how a count of "unused"
                // paths came out equal to the number of paths.
                "has_usage_cell": usage_present(&usage),
                "hits": hits,
                "last_hit_s": last,
                "declared_s": declared_s,
                // Hits on this one can never rise: it is read through plain
                // `idx_query`, which records nothing. Not a judgement about
                // the path — a statement about the instrument.
                "hits_not_recorded_for_this_path": is_orderpath,
            })
        })
        .collect();

    Json(serde_json::json!({
        "unserved": unserved,
        "declared": declared,
        "paths_without_a_usage_cell": no_cell,
    }))
    .into_response()
}

/// Whether the engine has a usage cell for a path at all.
///
/// Its own function so the `Option` is read once and reported honestly:
/// collapsing "no cell" into "zero hits" is what made the first version of
/// this route say every path was unused.
fn usage_present(u: &Option<(u64, i64, i64)>) -> bool {
    u.is_some()
}
