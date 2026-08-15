//! The per-user **message** shadow.
//!
//! Its counters were rewritten on 2026-08-02: `blob_ref_differs` compared
//! the two filenames, and once stage 6 blanked the shared one it was true
//! for all 31,416 rows. A check that cannot come out zero reports nothing.
//! What replaced it — `shared_still_names_a_file`, `per_user_unresolved` —
//! can.

use super::prelude::*;

/// `POST /v1/admin/maintenance:usermsg-shadow` — stage 3 of the per-user
/// message projection.
///
/// Compares, per user, what the shared thread index serves against what the
/// per-user index holds, and — for the messages in both — whether the two
/// `blob_ref`s agree and which of them actually resolves on disk.
///
/// The membership difference and the blob_ref difference answer different
/// questions, and the cutover needs both:
///
/// - `only_in_shared` is a message the shared index shows a user who has no
///   copy of it. Expected to equal the backfill's `not_this_users`.
/// - `only_in_per_user` must be zero. Anything here is the per-user index
///   inventing a message, which would be worse than the defect.
/// - `per_user_resolves` / `per_user_unresolved` measure the read that
///   actually happens now, against the disk rather than against the other
///   row.
/// - `shared_still_names_a_file` must be zero once stage 6 has run. It
///   replaced a `blob_ref_differs` that compared the two filenames — which
///   became true for every row the moment the shared one was blanked, and
///   a check that always fires reports nothing.
///
/// Read-only. See `.claude/rfcs/20260731-per-user-message-projection.md`.
pub(crate) async fn usermsg_shadow_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    // What the store did while this ran — see `store_motion`.
    let motion = crate::store_motion::begin(&state);
    use mailrs_core_api::method::message::MessageWire;

    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut threads = 0u64;
    let mut in_both = 0u64;
    let mut only_in_shared = 0u64;
    let mut only_in_per_user = 0u64;
    let mut shared_still_names_a_file = 0u64;
    let mut per_user_resolves = 0u64;
    let mut per_user_unresolved = 0u64;
    let mut only_shared_samples: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut named_samples: Vec<String> = Vec::new();
    let mut unresolved_samples: Vec<String> = Vec::new();
    let mut only_per_user_samples: Vec<String> = Vec::new();
    // A thread the user participates in whose per-user index is empty would
    // show nothing after the cutover where it shows something now. Correct
    // by the tenancy argument and still a visible regression, so it is
    // counted before the switch rather than discovered after it.
    let mut threads_empty_after = 0u64;
    // Per user, capped per user. A global cap of eight fills from whichever
    // account the iteration reaches first — every sample came back
    // `dmarc@golia.jp` while 113 of the 151 belonged to `lihao@golia.jp`,
    // so the samples described the smaller half and said nothing about the
    // larger one.
    let mut threads_empty_samples: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    // Per user, because eight samples taken in iteration order say nothing
    // about the other 143. Whether these are one account's monitoring
    // artefacts or somebody's mail is the whole decision.
    let mut empty_by_user: std::collections::BTreeMap<String, u64> =
        std::collections::BTreeMap::new();

    for user in &users {
        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            threads += 1;
            let mut shared: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for blob in state
                .mailbox
                .thread_messages_for_maintenance(&tid)
                .unwrap_or_default()
            {
                if let Ok(w) = serde_json::from_slice::<MessageWire>(&blob)
                    && !w.message_id.is_empty()
                {
                    shared.insert(w.message_id, w.blob_ref);
                }
            }
            let mine: std::collections::HashSet<String> = state
                .mailbox
                .user_thread_message_ids(user, &tid)
                .unwrap_or_default()
                .into_iter()
                .collect();

            for (mid, shared_ref) in &shared {
                match mine.contains(mid) {
                    false => {
                        only_in_shared += 1;
                        let per_user = only_shared_samples.entry(user.clone()).or_default();
                        if per_user.len() < 4 {
                            per_user.push(mid.clone());
                        }
                    }
                    true => {
                        in_both += 1;
                        let Ok(Some(facts)) = state.mailbox.user_message_facts(user, mid) else {
                            continue;
                        };
                        // Against the disk, and only about this user's own
                        // row: is their file there.
                        if read_maildir_file(user, &facts.blob_ref).is_some() {
                            per_user_resolves += 1;
                        } else {
                            per_user_unresolved += 1;
                            if unresolved_samples.len() < 8 {
                                unresolved_samples
                                    .push(format!("{user} {mid} mine={}", facts.blob_ref));
                            }
                        }
                        // Comparing the two `blob_ref`s stopped meaning
                        // anything when stage 6 blanked the shared one:
                        // every row "differs" now, and a check that always
                        // fires reports nothing. What is worth counting is
                        // whether a shared row still names a file at all.
                        if !shared_ref.is_empty() {
                            shared_still_names_a_file += 1;
                            if named_samples.len() < 8 {
                                named_samples.push(format!(
                                    "{user} {mid} shared={shared_ref} mine={}",
                                    facts.blob_ref
                                ));
                            }
                        }
                    }
                }
            }
            if mine.is_empty() && !shared.is_empty() {
                threads_empty_after += 1;
                *empty_by_user.entry(user.clone()).or_insert(0) += 1;
                let per_user = threads_empty_samples.entry(user.clone()).or_default();
                if per_user.len() < 4 {
                    per_user.push(format!("{tid} shared={}", shared.len()));
                }
            }
            for mid in &mine {
                if !shared.contains_key(mid) {
                    only_in_per_user += 1;
                    if only_per_user_samples.len() < 8 {
                        only_per_user_samples.push(format!("{user} {mid}"));
                    }
                }
            }
        }
    }

    Json(crate::store_motion::with_motion(
        serde_json::json!({
            "accounts": users.len(),
            "threads_compared": threads,
            "in_both": in_both,
            // Expected to match the backfill's `not_this_users`.
            "only_in_shared": only_in_shared,
            "only_in_shared_samples": only_shared_samples,
            // Must be zero — the per-user index inventing a message would be
            // worse than the defect it replaces.
            "only_in_per_user": only_in_per_user,
            "only_in_per_user_samples": only_per_user_samples,
            // Must be zero after `maintenance:strip-shared-per-user-fields`:
            // a shared row that still names a file is one a future fallback
            // could reach for.
            "shared_still_names_a_file": shared_still_names_a_file,
            "shared_still_names_a_file_samples": named_samples,
            // Measured against the disk, per owner.
            "per_user_resolves": per_user_resolves,
            "per_user_unresolved": per_user_unresolved,
            "per_user_unresolved_samples": unresolved_samples,
            // Threads that would render empty after the read cutover.
            "threads_empty_after_cutover": threads_empty_after,
            "threads_empty_after_cutover_samples": threads_empty_samples,
            "threads_empty_after_cutover_by_user": empty_by_user,
        }),
        motion.finish(&state),
    ))
    .into_response()
}
