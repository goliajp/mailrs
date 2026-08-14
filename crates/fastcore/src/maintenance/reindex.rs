//! Rebuild the index from the maildir — the operation the three sidecar
//! files exist for.
//!
//! Step 6 of `.claude/rfcs/20260814-the-maildir-is-the-store.md`. Steps 3
//! to 5 put UIDs, keyword bits and thread decisions beside the mail, and
//! the self-heal reads them back — **only on the branch that creates a
//! thread**, because that is the branch that was there to extend. On a
//! mailbox whose rows already exist, which is every mailbox on production,
//! none of it is read. The facts were durable and the rebuild was not.
//!
//! This walks every thread a user has and puts tier 1 back onto tier 2:
//! the keyword bits, the decision log, and the counters recomputed from
//! the per-user message rows.
//!
//! **Conditional, and it counts writes.** Every step compares before it
//! writes and reports what it changed, so a healthy mailbox reports zero —
//! a reconcile whose output cannot come out zero is not a verification
//! (`measure-before-you-cut-over`), and a counter incremented before the
//! write is how the read-state backfill reported repairing 215 rows it
//! never touched (`periodic-work-must-converge`).
//!
//! # What a dry run can and cannot tell you
//!
//! Every leg is evaluated — the first version skipped three of the four
//! and reported zero for them, which reads as a clean bill of health from
//! checks that never ran. But the legs are ordered and the last one is
//! **downstream**: the recount asks the message rows, and the flag replay
//! is what corrects them. So a dry run reports `counts_repaired` against
//! the rows as they are now and under-reports the recount that follows a
//! flag replay it did not perform. A dry run cannot predict the
//! consequences of changes it declined to make; the three independent
//! legs are exact.
//!
//! It does **not** drop the rows first. The RFC's phrasing is "drops a
//! user's tier-2 rows and rebuilds them", and reconciling in place is the
//! same destination with no window in which the mailbox is empty. What a
//! drop would additionally catch is a row that should not exist at all;
//! `maintenance:drop-empty-threads` already owns that.

use super::prelude::*;

#[derive(serde::Deserialize, Default)]
pub(crate) struct ReindexQuery {
    /// Restrict to one mailbox. Absent means every account.
    user: Option<String>,
    /// Report what would change without changing it.
    #[serde(default)]
    dry_run: bool,
}

/// `POST /v1/admin/maintenance:reindex[?user=&dry_run=true]`
pub(crate) async fn reindex_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<ReindexQuery>,
) -> axum::response::Response {
    let users = match &q.user {
        Some(u) => vec![u.clone()],
        None => match state.mailbox.list_account_addresses() {
            Ok(u) => u,
            Err(e) => {
                tracing::error!(err = %e, "list_account_addresses failed");
                return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        },
    };

    let mut walked = 0u64;
    let mut changed = 0u64;
    let mut from_keywords = 0u64;
    let mut from_flags = 0u64;
    let mut from_log = 0u64;
    let mut counts_repaired = 0u64;
    let mut by_user: std::collections::BTreeMap<String, serde_json::Value> = Default::default();

    for user in &users {
        let kw = crate::keywords::load(user);
        let md_root = crate::maildir_scan::mailbox_dir(user);
        let log = crate::threadstate::load(user);
        let mut u_walked = 0u64;
        let mut u_changed = 0u64;

        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            u_walked += 1;
            let mut touched = false;

            // The keyword bits. `None` means the mailbox has no meaning
            // recorded for the bit, which is a question that cannot be
            // asked rather than an answer of "no" — leaving the row alone
            // is the only honest response.
            for (name, want) in [
                (crate::keywords::ARCHIVED, "archived"),
                (crate::keywords::PINNED, "pinned"),
            ] {
                let Some(on_disk) = crate::keywords::thread_has(&state, &kw, user, &tid, name)
                else {
                    continue;
                };
                let Ok(Some(row)) = state.mailbox.get_thread_for_user(user, &tid) else {
                    continue;
                };
                let in_index = match want {
                    "archived" => row.archived,
                    _ => row.pinned,
                };
                if in_index == on_disk {
                    continue;
                }
                if q.dry_run {
                    from_keywords += 1;
                    touched = true;
                    continue;
                }
                let wrote = match want {
                    "archived" => state.mailbox.set_archived(user, &tid, on_disk),
                    _ => state.mailbox.set_pinned(user, &tid, on_disk),
                };
                match wrote {
                    Ok(_) => {
                        from_keywords += 1;
                        touched = true;
                    }
                    Err(e) => {
                        tracing::warn!(err = %e, %user, %tid, %name, "reindex: keyword replay failed")
                    }
                }
            }

            // Read state, which is tier 1 too: the `S` in the file name.
            //
            // **The file wins in both directions here**, which is what
            // makes this a rebuild rather than a repair. The read-state
            // backfill deliberately lets the index win when it is ahead,
            // on the argument that those reads were real; that argument
            // belongs to a one-time migration, not to an operation whose
            // whole definition is "tier 2 is derived from tier 1". On
            // production the two agree — `seen_only_in_index: 0`,
            // measured — so nothing is at stake in saying so plainly.
            if let Some(md_root) = md_root.as_ref() {
                for mid in state
                    .mailbox
                    .user_thread_message_ids(user, &tid)
                    .unwrap_or_default()
                {
                    let Ok(Some(facts)) = state.mailbox.user_message_facts(user, &mid) else {
                        continue;
                    };
                    let Some((md, id)) = mailrs_maildir::locate(md_root, &facts.blob_ref) else {
                        continue;
                    };
                    let Ok(Some(flags)) = md.flags_of(&id) else {
                        continue;
                    };
                    let on_disk = flags.contains(&mailrs_maildir::Flag::Seen);
                    if (facts.flags & mailrs_core_api::method::message::FLAG_SEEN != 0) == on_disk {
                        continue;
                    }
                    if q.dry_run {
                        from_flags += 1;
                        touched = true;
                        continue;
                    }
                    match state.mailbox.set_user_message_seen(user, &mid, on_disk) {
                        Ok(true) => {
                            from_flags += 1;
                            touched = true;
                        }
                        Ok(false) => {}
                        Err(e) => {
                            tracing::warn!(err = %e, %user, %mid, "reindex: flag replay failed")
                        }
                    }
                }
            }

            // The decision log: snooze, importance, requires_action, and
            // the category — through `set_bucket`, which owns the axes.
            let log_changes = if q.dry_run {
                crate::threadstate::would_change_row(&state, &log, user, &tid)
            } else {
                crate::threadstate::apply_to_row(&state, &log, user, &tid)
            };
            if log_changes {
                from_log += 1;
                touched = true;
            }

            // The counters, recomputed from the per-user message rows —
            // the derivation `unread_count` is, rather than the number it
            // was last patched to.
            let recount = if q.dry_run {
                state.mailbox.thread_counts_need_repair(user, &tid)
            } else {
                state.mailbox.repair_thread_counts(user, &tid)
            };
            match recount {
                Ok(true) => {
                    counts_repaired += 1;
                    touched = true;
                }
                Ok(false) => {}
                Err(e) => tracing::warn!(err = %e, %user, %tid, "reindex: recount failed"),
            }

            if touched {
                u_changed += 1;
            }
        }

        walked += u_walked;
        changed += u_changed;
        if u_changed > 0 {
            by_user.insert(
                user.clone(),
                serde_json::json!({ "walked": u_walked, "changed": u_changed }),
            );
        }
    }

    Json(serde_json::json!({
        "dry_run": q.dry_run,
        "accounts": users.len(),
        "threads_walked": walked,
        "threads_changed": changed,
        "from_keywords": from_keywords,
        "from_flags": from_flags,
        "from_threadstate": from_log,
        // Under-reports in a dry run: the recount is downstream of the
        // flag replay, which a dry run does not perform. See the module
        // docs.
        "counts_repaired": counts_repaired,
        "by_user": by_user,
    }))
    .into_response()
}
