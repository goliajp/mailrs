//! Counting routes: what is actually in the store right now.
//!
//! Separate from the shadows because they compare nothing — they answer
//! "how many", which is what tells a zero apart from an empty walk.

use super::prelude::*;

/// `POST /v1/admin/maintenance:tier-info` — what the store is holding
/// in RAM versus on disk.
///
/// `cold_keys` / `cold_bytes` are what tiering moved out; `stub_bytes`
/// is what that cost to keep addressable. With tiering off the budget
/// reads 0 and nothing is cold, which is the honest baseline to
/// compare against.
pub(crate) async fn tier_info_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let info = state.mailbox.store_ref().info();
    Json(serde_json::json!({
        "keys": info.keys,
        "used_memory": info.used_memory,
        "aof_bytes": info.aof_bytes,
        // None when tiering is off — the difference between "nothing
        // is cold" and "nothing can be cold" is worth keeping visible.
        "tier": info.tiering.map(|t| {
            serde_json::json!({
                "budget_bytes": t.tier_budget_bytes,
                "effective_target": t.tier_effective_target,
                "cold_keys": t.cold_keys,
                "cold_bytes": t.cold_bytes,
                "stub_bytes": t.stub_bytes,
                "index_reserved_bytes": t.index_reserved_bytes,
                "vlog_size_bytes": t.vlog_size_bytes,
            })
        }),
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:threaduser-census` — walk every
/// membership row and report which ones could not be indexed.
///
/// `TABLE.VERIFY` says how many entries an index holds; when that is
/// short of the row count it does not say *which* rows are missing. A
/// composite orderpath can only encode a row where every sort column is
/// present, so this counts rows by which column is empty.
pub(crate) async fn threaduser_census_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let store = state.mailbox.store_ref();
    let keys = store.keys(Some(b"mailrs:threaduser:*"), None);
    let mut total = 0u64;
    let mut empty: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut samples: Vec<String> = Vec::new();
    for k in &keys {
        total += 1;
        let pairs = match store.hgetall(k) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let map: std::collections::HashMap<&[u8], &[u8]> = pairs
            .iter()
            .map(|(f, v)| (f.as_slice(), v.as_slice()))
            .collect();
        let mut bad = false;
        for col in [
            "user", "tid", "bucket", "category", "activity", "starred", "archived",
        ] {
            if map
                .get(col.as_bytes())
                .map(|v| v.is_empty())
                .unwrap_or(true)
            {
                *empty.entry(col.to_string()).or_default() += 1;
                bad = true;
            }
        }
        if bad && samples.len() < 5 {
            samples.push(String::from_utf8_lossy(k).into_owned());
        }
    }
    Json(serde_json::json!({
        "rows": total,
        "empty_or_missing": empty,
        "samples": samples,
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:table-verify` — ask the engine whether
/// the access paths it maintains agree with the rows.
///
/// `drift` is the number that matters: non-zero means an index no
/// longer re-derives to what it stores, which is the failure this whole
/// migration exists to make impossible by hand. `coerce_failures` and
/// `type_mismatches` say a column's declared type does not match what
/// was written — a schema bug rather than a maintenance one.
///
/// kevy returns six unnamed counters per index; the names are recovered
/// here from the doc comment on `TableVerifyReport`.
pub(crate) async fn table_verify_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let name = q.get("table").map(String::as_str).unwrap_or("threaduser");
    // `table_verify_report`, not the positional `table_verify` it
    // supersedes: that one returned `[u64; 6]` and every field was read by
    // index here, so swapping `duplicates` and `drift` would have been
    // invisible. kevy 4.1 also split the counter that misled us — its
    // `coerce_failures` was a lifetime tally that counted absent columns
    // too, which is how a healthy migration read as 30,152 live failures.
    // `absent` is now its own number and not a fault.
    match state
        .mailbox
        .store_ref()
        .table_verify_report(name.as_bytes())
    {
        Ok(report) => {
            let indexes: Vec<serde_json::Value> = report
                .per_index
                .into_iter()
                .map(|i| {
                    serde_json::json!({
                        "index": String::from_utf8_lossy(&i.name),
                        "entries": i.entries,
                        "bytes": i.approx_bytes,
                        "rows_walked": i.rows,
                        // A row that derives a value and has no entry: the
                        // "a writer forgot this path" class, which the
                        // drift walk structurally cannot see because it
                        // iterates entries and a missing one is not there
                        // to iterate. This is the counter that would have
                        // caught `record_message_arrival` not writing the
                        // membership row.
                        "missing": i.missing,
                        "drift": i.drift,
                        "duplicates": i.duplicates,
                        "coerce_failures": i.coerce_failures,
                        // Excluded by design (NULL semantics) — not a fault.
                        "absent": i.absent,
                        // Dropped for exceeding MAX_STR_COMPONENT; two
                        // Message-IDs did exactly this once.
                        "excluded": i.excluded,
                    })
                })
                .collect();
            Json(serde_json::json!({
                "table": name,
                "indexes": indexes,
                "spot_check": {
                    "rows": report.spot_rows,
                    "type_mismatches": report.spot_type_mismatches,
                },
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!(err = %e, %name, "table_verify failed");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e}"),
            )
                .into_response()
        }
    }
}

/// `POST /v1/admin/maintenance:backfill-thread-user` — one segment of
/// the membership-row backfill for the declared `threaduser` table.
///
/// Call with `?user=<addr>&offset=<n>&limit=<n>`; omit `user` to get the
/// account list back and nothing else, which is how a driver discovers
/// what to iterate. Each call walks one page of that user's by_activity
/// zset and writes the membership row **only where it is absent or a
/// field differs**, so re-running over converged data reports
/// `written: 0` and costs one HGETALL per row rather than a write.
///
/// `done` is true when the page came back short, meaning the caller has
/// reached the end of this user's threads.
/// `POST /v1/admin/maintenance:legacy-zset-census`
///
/// Cardinality of every zset in `keys::all_user_thread_zsets` — the list
/// `drop-legacy-zsets` deletes — per account.
///
/// Read-only, and it exists because "is this reader looking at an empty
/// set?" was being answered by reasoning. `backfill-threading` enumerated
/// `user_threads_by_activity` and saw 9 messages against 30,562 declared
/// rows; `list_sent_messages` read `user_threads_sent`, which still holds
/// hundreds because the maildir sweep refills that one and nothing refills
/// the others. Those two facts together say the legacy zsets are alive to
/// different degrees, and several readers remain — in `importance.rs`,
/// `bayes_train.rs`, `backfill_decode.rs`. This turns that into numbers
/// before anything is concluded about them.
pub(crate) async fn legacy_zset_census_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let users = match q.get("user") {
        Some(u) => vec![u.clone()],
        None => state.mailbox.list_account_addresses().unwrap_or_default(),
    };
    let store = state.mailbox.store_ref();

    // key suffix -> total across accounts, so an all-zero row names a set
    // nothing writes any more.
    let mut totals: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut per_user = Vec::new();
    for user in &users {
        let mut row = serde_json::Map::new();
        let mut any = 0u64;
        for key in mailrs_mailbox_kevy::keys::all_user_thread_zsets(user) {
            let n = store.zcard(key.as_bytes()).unwrap_or(0) as u64;
            let label = key
                .rsplit_once(":threads:")
                .map(|(_, t)| t.to_string())
                .unwrap_or_else(|| key.clone());
            *totals.entry(label.clone()).or_default() += n;
            any += n;
            row.insert(label, serde_json::json!(n));
        }
        if any > 0 {
            row.insert("user".into(), serde_json::json!(user));
            per_user.push(serde_json::Value::Object(row));
        }
    }

    Json(serde_json::json!({
        "users_checked": users.len(),
        "totals": totals,
        "users_with_any": per_user,
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:spoof-landing` — of the messages that
/// failed DMARC while claiming one of our own domains, how many reached
/// the inbox rather than Junk.
///
/// There was no way to ask this. The verdict is a per-thread `category` in
/// this process's embedded kevy, and the evidence is an
/// `Authentication-Results` header in a maildir file; nothing outside the
/// process can join the two, and the maildir path does not distinguish Junk
/// (delivery is always to INBOX, and `target_folder` only ever reaches
/// `category`). So "40 of the last 55 DMARC failures forged our own
/// domains" was measurable from the filesystem on 2026-08-01 and "where
/// did they land" was not.
///
/// Counting is deliberately per user and per domain: a single total would
/// hide one account absorbing all of it, which is what the global sample
/// cap did to the per-user-message shadow report.
pub(crate) async fn spoof_landing_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    // Derived, not hard-coded: a domain added tomorrow is covered without
    // anyone remembering this route exists.
    let hosted: std::collections::HashSet<String> = state
        .mailbox
        .list_domains()
        .unwrap_or_default()
        .into_iter()
        .map(|(d, _)| d.to_ascii_lowercase())
        .collect();

    let mut messages_scanned = 0u64;
    let mut file_unreadable = 0u64;
    let mut dmarc_fail = 0u64;
    let mut forged_hosted = 0u64;
    // The answer, split by where it landed.
    let mut landed: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut by_domain: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut by_user: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut inbox_samples: Vec<String> = Vec::new();

    for user in &users {
        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            // This user's verdict: category is per-user state, and the
            // question here is where the mail landed *for them*.
            let category = match state.mailbox.get_thread_for_user(user, &tid) {
                Ok(Some(row)) => row.category,
                _ => continue,
            };
            for mid in state
                .mailbox
                .user_thread_message_ids(user, &tid)
                .unwrap_or_default()
            {
                let Ok(Some(facts)) = state.mailbox.user_message_facts(user, &mid) else {
                    continue;
                };
                messages_scanned += 1;
                let Some(raw) = read_maildir_file(user, &facts.blob_ref) else {
                    file_unreadable += 1;
                    continue;
                };
                // Headers only. Reading whole bodies over 30k messages to
                // find two header lines would make this unrunnable.
                let head_len = raw.len().min(8192);
                let head = String::from_utf8_lossy(&raw[..head_len]);
                if !head.to_ascii_lowercase().contains("dmarc=fail") {
                    continue;
                }
                dmarc_fail += 1;
                let Some(from_line) = head
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("from:"))
                else {
                    continue;
                };
                let Some(domain) = from_header_domains(from_line)
                    .into_iter()
                    .find(|d| hosted.contains(d))
                else {
                    continue;
                };
                forged_hosted += 1;
                *by_domain.entry(domain).or_default() += 1;
                *by_user.entry(user.clone()).or_default() += 1;
                *landed.entry(category.clone()).or_default() += 1;
                // Anything not filed as junk is a forgery of our own users
                // sitting in somebody's inbox, so name those specifically.
                if category != "spam" && category != "scam" && inbox_samples.len() < 20 {
                    inbox_samples.push(format!("{user} {tid} {mid} category={category}"));
                }
            }
        }
    }

    axum::Json(serde_json::json!({
        "accounts": users.len(),
        "hosted_domains": hosted.len(),
        "messages_scanned": messages_scanned,
        "file_unreadable": file_unreadable,
        "dmarc_fail": dmarc_fail,
        "forged_hosted_domain": forged_hosted,
        "landed_by_category": landed,
        "by_domain": by_domain,
        "by_user": by_user,
        "not_filed_as_junk_samples": inbox_samples,
    }))
    .into_response()
}
