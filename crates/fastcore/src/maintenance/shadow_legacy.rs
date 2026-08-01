//! The legacy-zset shadow: what the hand-maintained indexes hold against
//! what the declared table says. Its verdict is what let the zsets go.

use super::prelude::*;

/// `POST /v1/admin/maintenance:shadow-read` — compare the ORDERPATH's
/// answer with the zset's for every account, without serving either.
///
/// The zsets stay authoritative through this phase. This is the only
/// step that can show the two agree on **content and order** before a
/// read is cut over; `TABLE.VERIFY` proves the index matches the rows,
/// which is a different claim — the rows themselves could be wrong.
///
/// Divergence is reported per user rather than summed, because one
/// account disagreeing is a different problem from all of them.
pub(crate) async fn shadow_read_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let limit: usize = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(200);
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let store = state.mailbox.store_ref();
    let mut report = Vec::new();
    let mut total_divergent = 0u64;

    for user in &users {
        // The three remaining shapes: Sent (its own flag index), the
        // default recency axis (a pure ORDERPATH), and np (a merge of
        // two bucket ranges, the only order this code produces).
        for axis in ["sent", "default", "np"] {
            let zkey = match axis {
                "sent" => mailrs_mailbox_kevy::keys::user_threads_sent(user),
                _ => mailrs_mailbox_kevy::keys::user_threads_by_activity(user),
            };
            let zset: Vec<String> = if axis == "np" {
                let mut merged: Vec<(i64, String)> = Vec::new();
                for k in [
                    mailrs_mailbox_kevy::keys::user_threads_notifications(user),
                    mailrs_mailbox_kevy::keys::user_threads_promotions(user),
                ] {
                    if let Ok(e) = store.zrevrange(k.as_bytes(), 0, limit as i64 - 1) {
                        merged.extend(e.into_iter().filter_map(|(m, sc)| {
                            String::from_utf8(m).ok().map(|t| (sc as i64, t))
                        }));
                    }
                }
                merged.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
                merged.into_iter().map(|(_, t)| t).take(limit).collect()
            } else {
                match store.zrevrange(zkey.as_bytes(), 0, limit as i64 - 1) {
                    Ok(e) => e
                        .into_iter()
                        .filter_map(|(m, _)| String::from_utf8(m).ok())
                        .collect(),
                    Err(_) => continue,
                }
            };
            let table = match axis {
                "sent" => state.mailbox.list_thread_ids_by_flag_via_table(
                    user,
                    "is_sender",
                    limit,
                    0,
                    None,
                ),
                "default" => state
                    .mailbox
                    .list_thread_ids_by_activity_via_table(user, limit, None),
                _ => {
                    let mut m: Vec<String> = Vec::new();
                    for b in ["notifications", "promotions"] {
                        if let Ok(t) = state
                            .mailbox
                            .list_thread_ids_by_bucket_via_table(user, b, limit)
                        {
                            m.extend(t);
                        }
                    }
                    Ok(m)
                }
            };
            let Ok(table) = table else { continue };
            let zs: std::collections::BTreeSet<&String> = zset.iter().collect();
            let ts: std::collections::BTreeSet<&String> = table.iter().collect();
            let only_z = zs.difference(&ts).count();
            let only_t = ts.difference(&zs).count();
            // For Sent, show what the rows actually say about the
            // threads the zset claims and the table does not — the
            // 58-vs-9 gap on one account is unexplained and the
            // membership row is where the answer has to be.
            let detail: Vec<serde_json::Value> = if axis == "sent" {
                zs.difference(&ts)
                    .take(4)
                    .map(|tid| {
                        let key = mailrs_mailbox_kevy::keys::thread_user(user, tid);
                        let row = store.hgetall(key.as_bytes()).unwrap_or_default();
                        let f = |n: &str| -> Option<String> {
                            row.iter()
                                .find(|(k, _)| k == n.as_bytes())
                                .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
                        };
                        let th = store
                            .hgetall(mailrs_mailbox_kevy::keys::thread(tid).as_bytes())
                            .unwrap_or_default();
                        let tf = |n: &str| -> Option<String> {
                            th.iter()
                                .find(|(k, _)| k == n.as_bytes())
                                .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
                        };
                        serde_json::json!({
                            "tid": tid,
                            "row_exists": !row.is_empty(),
                            "row_is_sender": f("is_sender"),
                            "row_sent_only": f("sent_only"),
                            "thread_senders": tf("senders_csv"),
                            "thread_sent_count": tf("sent_count"),
                            "thread_count": tf("count"),
                        })
                    })
                    .collect()
            } else {
                Vec::new()
            };

            // np's table side is unsorted here (two concatenated
            // ranges); only membership is meaningful for it.
            let order_matches = axis == "np" || zset == table;
            if only_z > 0 || only_t > 0 || !order_matches {
                total_divergent += 1;
                report.push(serde_json::json!({
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
        }

        // Boolean predicate axes — each served from its own flag
        // index rather than a sort prefix.
        for (flag, zkey) in [
            (
                "starred",
                mailrs_mailbox_kevy::keys::user_threads_starred(user),
            ),
            (
                "archived",
                mailrs_mailbox_kevy::keys::user_threads_archived(user),
            ),
            (
                "pinned",
                mailrs_mailbox_kevy::keys::user_threads_pinned(user),
            ),
            (
                "unread",
                mailrs_mailbox_kevy::keys::user_threads_has_unread(user),
            ),
            (
                "has_action",
                mailrs_mailbox_kevy::keys::user_threads_has_action(user),
            ),
        ] {
            let zset: Vec<String> = match store.zrevrange(zkey.as_bytes(), 0, limit as i64 - 1) {
                Ok(e) => e
                    .into_iter()
                    .filter_map(|(m, _)| String::from_utf8(m).ok())
                    .collect(),
                Err(_) => continue,
            };
            let table = match state
                .mailbox
                .list_thread_ids_by_flag_via_table(user, flag, limit, 0, None)
            {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(err = %e, %user, flag, "flag index query failed");
                    continue;
                }
            };
            let zs: std::collections::BTreeSet<&String> = zset.iter().collect();
            let ts: std::collections::BTreeSet<&String> = table.iter().collect();
            let only_z = zs.difference(&ts).count();
            let only_t = ts.difference(&zs).count();
            let order_matches = zset == table;
            if only_z > 0 || only_t > 0 || !order_matches {
                total_divergent += 1;
                report.push(serde_json::json!({
                    "user": user,
                    "bucket": format!("flag:{flag}"),
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
        }

        // Category axis — the other declared ORDERPATH. Distinct from
        // the bucket axis: `bucket` is the folder a thread is filed
        // under, `category` is the classifier's verdict, and the two
        // use different vocabularies (spam vs junk, notification vs
        // notifications).
        for cat in ["inbox", "notification", "promotion", "spam"] {
            let zkey = mailrs_mailbox_kevy::keys::user_threads_by_category(user, cat);
            let zset: Vec<String> = match store.zrevrange(zkey.as_bytes(), 0, limit as i64 - 1) {
                Ok(e) => e
                    .into_iter()
                    .filter_map(|(m, _)| String::from_utf8(m).ok())
                    .collect(),
                Err(_) => continue,
            };
            let table = match state
                .mailbox
                .list_thread_ids_by_category_via_table(user, cat, limit)
            {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(err = %e, %user, cat, "category orderpath query failed");
                    continue;
                }
            };
            let zs: std::collections::BTreeSet<&String> = zset.iter().collect();
            let ts: std::collections::BTreeSet<&String> = table.iter().collect();
            let only_z = zs.difference(&ts).count();
            let only_t = ts.difference(&zs).count();
            let order_matches = zset == table;
            if only_z > 0 || only_t > 0 || !order_matches {
                total_divergent += 1;
                report.push(serde_json::json!({
                    "user": user,
                    "bucket": format!("cat:{cat}"),
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
        }

        for (bucket, zkey) in [
            ("inbox", mailrs_mailbox_kevy::keys::user_threads_inbox(user)),
            ("junk", mailrs_mailbox_kevy::keys::user_threads_junk(user)),
            (
                "notifications",
                mailrs_mailbox_kevy::keys::user_threads_notifications(user),
            ),
            (
                "promotions",
                mailrs_mailbox_kevy::keys::user_threads_promotions(user),
            ),
        ] {
            let zset: Vec<String> = match store.zrevrange(zkey.as_bytes(), 0, limit as i64 - 1) {
                Ok(e) => e
                    .into_iter()
                    .filter_map(|(m, _)| String::from_utf8(m).ok())
                    .collect(),
                Err(e) => {
                    tracing::warn!(err = %e, %user, bucket, "zrevrange failed");
                    continue;
                }
            };
            // Inbox is served from the sent-excluding ORDERPATH, so
            // compare against that one — otherwise this reports a
            // divergence the serving path does not have.
            let table_result = if bucket == "inbox" {
                state
                    .mailbox
                    .list_thread_ids_by_bucket_unsent_via_table(user, bucket, limit)
            } else {
                state
                    .mailbox
                    .list_thread_ids_by_bucket_via_table(user, bucket, limit)
            };
            let table = match table_result {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(err = %e, %user, bucket, "orderpath query failed");
                    continue;
                }
            };

            // Both sides capped at `limit`, so a full page on either
            // side means the tails were never compared — the sets can
            // differ purely from where each was cut. Say so rather than
            // reporting a divergence the data does not support.
            let truncated = zset.len() >= limit || table.len() >= limit;

            let zs: std::collections::BTreeSet<&String> = zset.iter().collect();
            let ts: std::collections::BTreeSet<&String> = table.iter().collect();
            let only_zset: Vec<&String> = zs.difference(&ts).copied().collect();
            let only_table: Vec<&String> = ts.difference(&zs).copied().collect();
            let order_matches = zset == table;

            // When the sets agree but the order does not, the useful
            // question is where they first part and whether the two
            // threads there share an activity timestamp — a tie the
            // two sides break differently is harmless, a genuine
            // ordering difference is not.
            let order_diff = if order_matches {
                serde_json::Value::Null
            } else {
                let at = zset.iter().zip(table.iter()).position(|(a, b)| a != b);
                match at {
                    Some(i) => {
                        let act = |tid: &String| -> Option<String> {
                            let key = mailrs_mailbox_kevy::keys::thread_user(user, tid);
                            store.hgetall(key.as_bytes()).ok().and_then(|row| {
                                row.iter()
                                    .find(|(f, _)| f == b"activity")
                                    .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
                            })
                        };
                        serde_json::json!({
                            "at_index": i,
                            "zset_activity": act(&zset[i]),
                            "table_activity": act(&table[i]),
                        })
                    }
                    None => serde_json::json!({ "at_index": "length only" }),
                }
            };

            // A thread the zset claims and the table does not is the
            // only shape that could lose data on cutover. Report what
            // the membership row actually says about it, so a stale
            // zset entry is distinguishable from a missing row.
            let missing: Vec<serde_json::Value> = only_zset
                .iter()
                .take(5)
                .map(|tid| {
                    let key = mailrs_mailbox_kevy::keys::thread_user(user, tid);
                    let row = store.hgetall(key.as_bytes()).unwrap_or_default();
                    let field = |name: &str| -> Option<String> {
                        row.iter()
                            .find(|(f, _)| f == name.as_bytes())
                            .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
                    };
                    serde_json::json!({
                        "tid": tid,
                        "row_exists": !row.is_empty(),
                        "row_bucket": field("bucket"),
                        "row_category": field("category"),
                    })
                })
                .collect();

            // Symmetric detail for the other direction: what the table
            // has and the zset does not. On the Inbox axis this is the
            // question of whether the rows include sent-only threads
            // the zset deliberately excludes.
            let extra: Vec<serde_json::Value> = only_table
                .iter()
                .take(5)
                .map(|tid| {
                    let key = mailrs_mailbox_kevy::keys::thread_user(user, tid);
                    let row = store.hgetall(key.as_bytes()).unwrap_or_default();
                    let field = |name: &str| -> Option<String> {
                        row.iter()
                            .find(|(f, _)| f == name.as_bytes())
                            .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
                    };
                    serde_json::json!({
                        "tid": tid,
                        "sent": field("sent"),
                        "category": field("category"),
                        "archived": field("archived"),
                    })
                })
                .collect();

            if !only_zset.is_empty() || !only_table.is_empty() || !order_matches {
                total_divergent += 1;
                report.push(serde_json::json!({
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
        }
    }

    Json(serde_json::json!({
        "limit": limit,
        "axes_checked": users.len() * 16,
        "axes_divergent": total_divergent,
        "divergences": report,
    }))
    .into_response()
}
