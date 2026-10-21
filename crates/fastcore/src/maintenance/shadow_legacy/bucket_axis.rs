//! The bucket axes, which report more than membership: where the order
//! first parts, and what the rows say about each thread on one side only.

use std::collections::BTreeSet;

use kevy_embedded::Store;
use mailrs_mailbox_kevy::{ArchiveScope, keys};
use serde_json::Value;

use super::axes::{Row, field, zset_ids};
use crate::FastcoreState;

pub(super) fn bucket_axis(
    state: &FastcoreState,
    store: &Store,
    user: &str,
    bucket: &str,
    zkey: &str,
    limit: usize,
) -> Option<Value> {
    let zset: Vec<String> = match store.zrevrange(zkey.as_bytes(), 0, limit as i64 - 1) {
        Ok(e) => zset_ids(e),
        Err(e) => {
            tracing::warn!(err = %e, %user, bucket, "zrevrange failed");
            return None;
        }
    };
    let table = served_bucket_ids(state, user, bucket, limit)?;

    // Both sides capped at `limit`, so a full page on either
    // side means the tails were never compared — the sets can
    // differ purely from where each was cut. Say so rather than
    // reporting a divergence the data does not support.
    let truncated = zset.len() >= limit || table.len() >= limit;

    let zs: BTreeSet<&String> = zset.iter().collect();
    let ts: BTreeSet<&String> = table.iter().collect();
    let only_zset: Vec<&String> = zs.difference(&ts).copied().collect();
    let only_table: Vec<&String> = ts.difference(&zs).copied().collect();
    let order_matches = zset == table;

    let order_diff = first_order_difference(store, user, &zset, &table, order_matches);

    // A thread the zset claims and the table does not is the
    // only shape that could lose data on cutover. Report what
    // the membership row actually says about it, so a stale
    // zset entry is distinguishable from a missing row.
    let missing: Vec<Value> = only_zset
        .iter()
        .take(5)
        .map(|tid| {
            let row = membership_row(store, user, tid);
            serde_json::json!({
                "tid": tid,
                "row_exists": !row.is_empty(),
                "row_bucket": field(&row, "bucket"),
                "row_category": field(&row, "category"),
            })
        })
        .collect();

    // Symmetric detail for the other direction: what the table
    // has and the zset does not. On the Inbox axis this is the
    // question of whether the rows include sent-only threads
    // the zset deliberately excludes.
    let extra: Vec<Value> = only_table
        .iter()
        .take(5)
        .map(|tid| {
            let row = membership_row(store, user, tid);
            serde_json::json!({
                "tid": tid,
                "sent": field(&row, "sent"),
                "category": field(&row, "category"),
                "archived": field(&row, "archived"),
            })
        })
        .collect();

    if !only_zset.is_empty() || !only_table.is_empty() || !order_matches {
        return Some(serde_json::json!({
            "user": user,
            "bucket": bucket,
            "zset_len": zset.len(),
            "table_len": table.len(),
            "truncated": truncated,
            "only_in_zset": only_zset.len(),
            "only_in_table": only_table.len(),
            "order_matches": order_matches,
            "order_diff": order_diff,
            "zset_only_rows": missing,
            "table_only_rows": extra,
        }));
    }
    None
}

/// The table side as the serving path reads it; `None` when it failed.
fn served_bucket_ids(
    state: &FastcoreState,
    user: &str,
    bucket: &str,
    limit: usize,
) -> Option<Vec<String>> {
    // Inbox is served from the sent-excluding ORDERPATH, so
    // compare against that one — otherwise this reports a
    // divergence the serving path does not have.
    let table_result = if bucket == "inbox" {
        state.mailbox.list_thread_ids_by_bucket_unsent_via_table(
            user,
            bucket,
            ArchiveScope::All,
            limit,
        )
    } else {
        state
            .mailbox
            .list_thread_ids_by_bucket_via_table(user, bucket, ArchiveScope::All, limit)
    };
    match table_result {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::warn!(err = %e, %user, bucket, "orderpath query failed");
            None
        }
    }
}

fn membership_row(store: &Store, user: &str, tid: &str) -> Row {
    let key = keys::thread_user(user, tid);
    store.hgetall(key.as_bytes()).unwrap_or_default()
}

// When the sets agree but the order does not, the useful
// question is where they first part and whether the two
// threads there share an activity timestamp — a tie the
// two sides break differently is harmless, a genuine
// ordering difference is not.
fn first_order_difference(
    store: &Store,
    user: &str,
    zset: &[String],
    table: &[String],
    order_matches: bool,
) -> Value {
    if order_matches {
        return serde_json::Value::Null;
    }
    let at = zset.iter().zip(table.iter()).position(|(a, b)| a != b);
    match at {
        Some(i) => {
            let act = |tid: &String| -> Option<String> {
                let key = keys::thread_user(user, tid);
                store
                    .hgetall(key.as_bytes())
                    .ok()
                    .and_then(|row| field(&row, "activity"))
            };
            serde_json::json!({
                "at_index": i,
                "zset_activity": act(&zset[i]),
                "table_activity": act(&table[i]),
            })
        }
        None => serde_json::json!({ "at_index": "length only" }),
    }
}
