//! The read-state shadow: the maildir's flags against the index's belief.
//!
//! Step 1 of `.claude/rfcs/20260814-the-maildir-is-the-store.md`, and
//! read-only. Nothing is written here — the point is to have the number
//! before anything moves, because the first number in a comparison like
//! this is usually a backfill gap rather than the defect, and cutting a
//! read over on it once nearly shipped a bigger fault than the one being
//! repaired (`measure-before-you-cut-over`).
//!
//! What it compares, per user, per message:
//!
//! - the file name's flags, which is what an IMAP client sees — `S` for
//!   read, `F` for starred, parsed by the stone from the `:2,FLAGS` suffix;
//! - the per-user message row's `flags` bitmask, which is what the web
//!   sees, and from which `unread_count` and the declared `unread` axis are
//!   derived.
//!
//! Every counter can come out zero on a healthy user. The two that matter
//! are directional and both are expected to be **non-zero today**, because
//! neither side writes the other:
//!
//! - `seen_only_on_disk` — read in an IMAP client; the web still shows it
//!   unread.
//! - `seen_only_in_index` — read in the web; an IMAP client still shows it
//!   unseen.

use std::collections::{BTreeMap, HashMap};

use mailrs_core_api::method::message::{FLAG_FLAGGED, FLAG_SEEN};
use mailrs_maildir::{Flag, Maildir};

use super::prelude::*;

/// The base message id a `blob_ref` names: no `.Folder/` prefix, no
/// `:2,FLAGS` suffix. Both are stripped because the id is what survives a
/// rename, and a rename is exactly what happens when a message is read.
fn base_id(blob_ref: &str) -> &str {
    let file = blob_ref.rsplit('/').next().unwrap_or(blob_ref);
    file.split(':').next().unwrap_or(file)
}

/// Every message file the user owns, across INBOX and every Maildir++
/// subfolder, as `base id -> flags on disk`.
///
/// One filesystem pass per mailbox rather than a `locate` per message:
/// `locate` would be correct but is O(mailbox) per lookup, and a user with
/// thirty thousand messages makes that quadratic.
fn flags_on_disk(state: &Arc<FastcoreState>, user: &str) -> HashMap<String, Vec<Flag>> {
    let mut out = HashMap::new();
    for mb in crate::imap::backend::list_mailboxes(state, user) {
        let md = Maildir::open(&mb.path);
        let entries = md
            .scan_new()
            .unwrap_or_default()
            .into_iter()
            .chain(md.scan_cur().unwrap_or_default());
        for e in entries {
            out.insert(e.id.0, e.flags);
        }
    }
    out
}

/// `POST /v1/admin/maintenance:read-state-shadow` — read-only.
pub(crate) async fn read_state_shadow_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut compared = 0u64;
    let mut seen_agrees = 0u64;
    let mut seen_only_on_disk = 0u64;
    let mut seen_only_in_index = 0u64;
    let mut flagged_only_on_disk = 0u64;
    let mut flagged_only_in_index = 0u64;
    // Two different causes, kept apart after the first run reported 215
    // under one name and every sample came back as the empty string —
    // which meant none of them were dangling references at all, they were
    // rows carrying no reference. One number covering both is a number
    // nobody can act on.
    //
    // `no_blob_ref` may well be legitimate (a row with no maildir file of
    // its own). `blob_ref_names_no_file` is a dangling reference and must
    // be zero: it would mean the web offers to open a message that cannot
    // be read.
    let mut index_row_has_no_blob_ref = 0u64;
    let mut blob_ref_names_no_file = 0u64;
    // Threads whose stored `unread_count` disagrees with the count derived
    // from the file names. This is the number the badge and the list draw,
    // so it is the user-visible form of the two directional counters.
    let mut threads_compared = 0u64;
    let mut unread_count_agrees = 0u64;
    let mut unread_count_differs = 0u64;

    // Per user, capped per user. A global cap fills from whichever account
    // the iteration reaches first, and then the samples describe that
    // account and say nothing about the rest — which is how an earlier
    // shadow's eight samples all came from one 38-row account while 113
    // rows belonged to another.
    let mut seen_disk_samples: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut seen_index_samples: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut no_ref_samples: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut dangling_samples: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut count_samples: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut by_user: BTreeMap<String, serde_json::Value> = BTreeMap::new();

    let push = |m: &mut BTreeMap<String, Vec<String>>, user: &str, s: String| {
        let per_user = m.entry(user.to_string()).or_default();
        if per_user.len() < 4 {
            per_user.push(s);
        }
    };

    for user in &users {
        let disk = flags_on_disk(&state, user);
        let mut u_disk = 0u64;
        let mut u_index = 0u64;
        let mut u_counts = 0u64;

        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            threads_compared += 1;
            let mut unread_on_disk = 0i64;

            for mid in state
                .mailbox
                .user_thread_message_ids(user, &tid)
                .unwrap_or_default()
            {
                let Ok(Some(facts)) = state.mailbox.user_message_facts(user, &mid) else {
                    continue;
                };
                if facts.blob_ref.is_empty() {
                    index_row_has_no_blob_ref += 1;
                    push(&mut no_ref_samples, user, mid.clone());
                    continue;
                }
                let Some(disk_flags) = disk.get(base_id(&facts.blob_ref)) else {
                    blob_ref_names_no_file += 1;
                    // The ref, not the message id: a dangling reference is
                    // diagnosed by looking for the file it names.
                    push(&mut dangling_samples, user, facts.blob_ref.clone());
                    continue;
                };
                compared += 1;

                let disk_seen = disk_flags.contains(&Flag::Seen);
                let index_seen = facts.flags & FLAG_SEEN != 0;
                if !disk_seen {
                    unread_on_disk += 1;
                }
                match (disk_seen, index_seen) {
                    (true, true) | (false, false) => seen_agrees += 1,
                    (true, false) => {
                        seen_only_on_disk += 1;
                        u_disk += 1;
                        push(&mut seen_disk_samples, user, mid.clone());
                    }
                    (false, true) => {
                        seen_only_in_index += 1;
                        u_index += 1;
                        push(&mut seen_index_samples, user, mid.clone());
                    }
                }

                let disk_flagged = disk_flags.contains(&Flag::Flagged);
                let index_flagged = facts.flags & FLAG_FLAGGED != 0;
                match (disk_flagged, index_flagged) {
                    (true, false) => flagged_only_on_disk += 1,
                    (false, true) => flagged_only_in_index += 1,
                    _ => {}
                }
            }

            // `unread_count` is a derivation of the `S` bits; comparing it
            // against the disk-derived count is the same fact at the
            // granularity a person actually sees.
            match state.mailbox.get_thread_for_user(user, &tid) {
                Ok(Some(row)) if row.unread_count == unread_on_disk => unread_count_agrees += 1,
                Ok(Some(row)) => {
                    unread_count_differs += 1;
                    u_counts += 1;
                    push(
                        &mut count_samples,
                        user,
                        format!("{tid} index={} disk={unread_on_disk}", row.unread_count),
                    );
                }
                _ => {}
            }
        }

        if u_disk > 0 || u_index > 0 || u_counts > 0 {
            by_user.insert(
                user.clone(),
                serde_json::json!({
                    "seen_only_on_disk": u_disk,
                    "seen_only_in_index": u_index,
                    "unread_count_differs": u_counts,
                }),
            );
        }
    }

    Json(serde_json::json!({
        "messages_compared": compared,
        "seen_agrees": seen_agrees,
        "seen_only_on_disk": seen_only_on_disk,
        "seen_only_in_index": seen_only_in_index,
        "flagged_only_on_disk": flagged_only_on_disk,
        "flagged_only_in_index": flagged_only_in_index,
        "index_row_has_no_blob_ref": index_row_has_no_blob_ref,
        "blob_ref_names_no_file": blob_ref_names_no_file,
        "threads_compared": threads_compared,
        "unread_count_agrees": unread_count_agrees,
        "unread_count_differs": unread_count_differs,
        "by_user": by_user,
        "samples": {
            "seen_only_on_disk": seen_disk_samples,
            "seen_only_in_index": seen_index_samples,
            "index_row_has_no_blob_ref": no_ref_samples,
            "blob_ref_names_no_file": dangling_samples,
            "unread_count_differs": count_samples,
        },
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::base_id;

    /// A `blob_ref` is stored at ingest, before the message has been read.
    /// Reading it renames the file into `cur/` with a `:2,S` suffix, so the
    /// only part that survives is the base id — which is what the disk map
    /// is keyed by. Getting this wrong would make every read message look
    /// like a missing file.
    #[test]
    fn a_base_id_survives_a_subfolder_and_a_flag_suffix() {
        assert_eq!(base_id("1786650987.M1P1.host"), "1786650987.M1P1.host");
        assert_eq!(base_id("1786650987.M1P1.host:2,S"), "1786650987.M1P1.host");
        assert_eq!(
            base_id(".Archive/1786650987.M1P1.host:2,SF"),
            "1786650987.M1P1.host"
        );
        assert_eq!(base_id(""), "");
    }
}
