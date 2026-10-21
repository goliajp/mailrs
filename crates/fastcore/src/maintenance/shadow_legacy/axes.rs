//! One account's comparison, axis by axis: each function answers the
//! divergence report row for one axis, or `None` when the two sides agree
//! or one side could not be read.

use std::collections::BTreeSet;

use kevy_embedded::Store;
use mailrs_mailbox_kevy::{ArchiveScope, keys};
use serde_json::Value;

use super::bucket_axis::bucket_axis;
use crate::FastcoreState;

pub(super) type Row = Vec<(Vec<u8>, Vec<u8>)>;

pub(super) fn divergences_for_user(
    state: &FastcoreState,
    store: &Store,
    user: &str,
    limit: usize,
) -> Vec<Value> {
    let mut found = Vec::new();
    // The three remaining shapes: Sent (its own flag index), the
    // default recency axis (a pure ORDERPATH), and np (a merge of
    // two bucket ranges, the only order this code produces).
    for axis in ["sent", "default", "np"] {
        found.extend(shape_axis(state, store, user, axis, limit));
    }
    // Boolean predicate axes — each served from its own flag
    // index rather than a sort prefix.
    for (flag, zkey) in flag_zsets(user) {
        found.extend(flag_axis(state, store, user, flag, &zkey, limit));
    }
    // Category axis — the other declared ORDERPATH. Distinct from
    // the bucket axis: `bucket` is the folder a thread is filed
    // under, `category` is the classifier's verdict, and the two
    // use different vocabularies (spam vs junk, notification vs
    // notifications).
    for cat in ["inbox", "notification", "promotion", "spam"] {
        found.extend(category_axis(state, store, user, cat, limit));
    }
    for (bucket, zkey) in [
        ("inbox", keys::user_threads_inbox(user)),
        ("junk", keys::user_threads_junk(user)),
        ("notifications", keys::user_threads_notifications(user)),
        ("promotions", keys::user_threads_promotions(user)),
    ] {
        found.extend(bucket_axis(state, store, user, bucket, &zkey, limit));
    }
    found
}

fn flag_zsets(user: &str) -> [(&'static str, String); 5] {
    [
        ("starred", keys::user_threads_starred(user)),
        ("archived", keys::user_threads_archived(user)),
        ("pinned", keys::user_threads_pinned(user)),
        ("unread", keys::user_threads_has_unread(user)),
        ("has_action", keys::user_threads_has_action(user)),
    ]
}

pub(super) fn zset_ids(entries: Vec<(Vec<u8>, f64)>) -> Vec<String> {
    entries
        .into_iter()
        .filter_map(|(m, _)| String::from_utf8(m).ok())
        .collect()
}

pub(super) fn field(row: &Row, name: &str) -> Option<String> {
    row.iter()
        .find(|(k, _)| k == name.as_bytes())
        .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
}

fn shape_axis(
    state: &FastcoreState,
    store: &Store,
    user: &str,
    axis: &str,
    limit: usize,
) -> Option<Value> {
    let zset = legacy_shape_zset(store, user, axis, limit)?;
    let Ok(table) = table_shape_ids(state, user, axis, limit) else {
        return None;
    };
    let zs: BTreeSet<&String> = zset.iter().collect();
    let ts: BTreeSet<&String> = table.iter().collect();
    let only_z = zs.difference(&ts).count();
    let only_t = ts.difference(&zs).count();
    // For Sent, show what the rows actually say about the
    // threads the zset claims and the table does not — the
    // 58-vs-9 gap on one account is unexplained and the
    // membership row is where the answer has to be.
    let detail: Vec<Value> = if axis == "sent" {
        zs.difference(&ts)
            .take(4)
            .map(|tid| sent_row_detail(store, user, tid))
            .collect()
    } else {
        Vec::new()
    };

    // np's table side is unsorted here (two concatenated
    // ranges); only membership is meaningful for it.
    let order_matches = axis == "np" || zset == table;
    if only_z > 0 || only_t > 0 || !order_matches {
        return Some(serde_json::json!({
            "user": user,
            "bucket": format!("axis:{axis}"),
            "zset_len": zset.len(),
            "table_len": table.len(),
            "truncated": zset.len() >= limit || table.len() >= limit,
            "only_in_zset": only_z,
            "only_in_table": only_t,
            "order_matches": order_matches,
            "zset_only_rows": detail,
            "table_only_rows": [],
            "order_diff": serde_json::Value::Null,
        }));
    }
    None
}

/// The zset side of a shape axis; `None` when it could not be read.
fn legacy_shape_zset(store: &Store, user: &str, axis: &str, limit: usize) -> Option<Vec<String>> {
    let zkey = match axis {
        "sent" => keys::user_threads_sent(user),
        _ => keys::user_threads_by_activity(user),
    };
    if axis == "np" {
        let mut merged: Vec<(i64, String)> = Vec::new();
        for k in [
            keys::user_threads_notifications(user),
            keys::user_threads_promotions(user),
        ] {
            if let Ok(e) = store.zrevrange(k.as_bytes(), 0, limit as i64 - 1) {
                merged.extend(
                    e.into_iter()
                        .filter_map(|(m, sc)| String::from_utf8(m).ok().map(|t| (sc as i64, t))),
                );
            }
        }
        merged.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        return Some(merged.into_iter().map(|(_, t)| t).take(limit).collect());
    }
    match store.zrevrange(zkey.as_bytes(), 0, limit as i64 - 1) {
        Ok(e) => Some(zset_ids(e)),
        Err(_) => None,
    }
}

fn table_shape_ids(
    state: &FastcoreState,
    user: &str,
    axis: &str,
    limit: usize,
) -> std::io::Result<Vec<String>> {
    match axis {
        "sent" => {
            state
                .mailbox
                .list_thread_ids_by_flag_via_table(user, "is_sender", limit, 0, None)
        }
        "default" => state.mailbox.list_thread_ids_by_activity_via_table(
            user,
            ArchiveScope::All,
            limit,
            None,
        ),
        _ => {
            let mut m: Vec<String> = Vec::new();
            for b in ["notifications", "promotions"] {
                if let Ok(t) = state.mailbox.list_thread_ids_by_bucket_via_table(
                    user,
                    b,
                    ArchiveScope::All,
                    limit,
                ) {
                    m.extend(t);
                }
            }
            Ok(m)
        }
    }
}

fn sent_row_detail(store: &Store, user: &str, tid: &String) -> Value {
    let key = keys::thread_user(user, tid);
    let row = store.hgetall(key.as_bytes()).unwrap_or_default();
    let th = store
        .hgetall(keys::thread(tid).as_bytes())
        .unwrap_or_default();
    serde_json::json!({
        "tid": tid,
        "row_exists": !row.is_empty(),
        "row_is_sender": field(&row, "is_sender"),
        "row_sent_only": field(&row, "sent_only"),
        "thread_senders": field(&th, "senders_csv"),
        "thread_sent_count": field(&th, "sent_count"),
        "thread_count": field(&th, "count"),
    })
}

fn flag_axis(
    state: &FastcoreState,
    store: &Store,
    user: &str,
    flag: &str,
    zkey: &str,
    limit: usize,
) -> Option<Value> {
    let zset = match store.zrevrange(zkey.as_bytes(), 0, limit as i64 - 1) {
        Ok(e) => zset_ids(e),
        Err(_) => return None,
    };
    let table = match state
        .mailbox
        .list_thread_ids_by_flag_via_table(user, flag, limit, 0, None)
    {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(err = %e, %user, flag, "flag index query failed");
            return None;
        }
    };
    plain_divergence(user, format!("flag:{flag}"), &zset, &table, limit)
}

fn category_axis(
    state: &FastcoreState,
    store: &Store,
    user: &str,
    cat: &str,
    limit: usize,
) -> Option<Value> {
    let zkey = keys::user_threads_by_category(user, cat);
    let zset = match store.zrevrange(zkey.as_bytes(), 0, limit as i64 - 1) {
        Ok(e) => zset_ids(e),
        Err(_) => return None,
    };
    let table = match state.mailbox.list_thread_ids_by_category_via_table(
        user,
        cat,
        ArchiveScope::All,
        limit,
    ) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(err = %e, %user, cat, "category orderpath query failed");
            return None;
        }
    };
    plain_divergence(user, format!("cat:{cat}"), &zset, &table, limit)
}

/// The report row for an axis compared on membership and order alone.
fn plain_divergence(
    user: &str,
    bucket: String,
    zset: &[String],
    table: &[String],
    limit: usize,
) -> Option<Value> {
    let zs: BTreeSet<&String> = zset.iter().collect();
    let ts: BTreeSet<&String> = table.iter().collect();
    let only_z = zs.difference(&ts).count();
    let only_t = ts.difference(&zs).count();
    let order_matches = zset == table;
    if only_z > 0 || only_t > 0 || !order_matches {
        return Some(serde_json::json!({
            "user": user,
            "bucket": bucket,
            "zset_len": zset.len(),
            "table_len": table.len(),
            "truncated": zset.len() >= limit || table.len() >= limit,
            "only_in_zset": only_z,
            "only_in_table": only_t,
            "order_matches": order_matches,
            "zset_only_rows": [],
            "table_only_rows": [],
            "order_diff": serde_json::Value::Null,
        }));
    }
    None
}
