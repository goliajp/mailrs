//! Re-derive the stored sender verdict from each message's own file.
//!
//! `sender_trust` is computed once, at ingest, and stored on the shared
//! message blob. So a change to how the verdict is folded reaches new
//! mail immediately and reaches stored mail never — and self-heal does
//! not close the gap, because it only writes rows that are *missing*,
//! never rows that are wrong.
//!
//! That asymmetry is the whole subject of the change this exists for
//! (`.claude/rfcs/20260816-a-check-mark-is-an-identity-claim.md`): five
//! production messages whose display name is a brand written backwards
//! behind a right-to-left override were stored as `verified`, because
//! every authentication check passed — and they did pass, on a domain
//! the attacker owns. Without this route the fix would warn about the
//! next one and stay silent about those five, which is the same shape as
//! the defect.
//!
//! # What it does not do
//!
//! It does not move mail, retag it, or touch read state. The verdict is
//! one string on one row; everything downstream reads it rather than
//! caching it.
//!
//! Idempotent, and it reports what it **walked** apart from what it
//! changed, so a second run says `changed: 0` against an unchanged
//! `copies_walked` rather than looking like it found nothing to do.
//!
//! # One directory pass per mailbox, not one per message
//!
//! `read_maildir_file` resolves a name through `Maildir::fetch`, which
//! falls back to `find_in_cur` — a full `read_dir` of `cur/` compared
//! linearly. Called once per message that is 33,583 directory reads
//! against a directory holding ~16,000 entries, and the first attempt at
//! this route sat at 100% of one core for eleven minutes without
//! finishing. So the mailbox is read **once** into a base-id → path map
//! and every lookup is a hash hit, the same shape
//! `backfill_read_state` uses for the same reason.

use std::collections::{BTreeMap, HashMap};

use super::prelude::*;

#[derive(serde::Deserialize, Default)]
pub(crate) struct RecomputeQuery {
    /// Report what would change without changing it.
    #[serde(default)]
    dry_run: bool,
}

/// `POST /v1/admin/maintenance:recompute-sender-trust?dry_run=true`
pub(crate) async fn recompute_sender_trust_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RecomputeQuery>,
) -> axum::response::Response {
    let motion = crate::store_motion::begin(&state);
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Per **user copy**, not per message: `sender_trust` lives on the
    // shared blob, so a message two people hold is examined twice. The
    // second look finds the first look's write and counts as agreement,
    // which is right — but the total is copies, and the name says so.
    let mut walked = 0u64;
    let mut agreed = 0u64;
    let mut changed = 0u64;
    let mut no_file = 0u64;
    // Copies whose own file has nothing to say about a verdict that is
    // already recorded. See `overwrites`.
    let mut silent = 0u64;
    let mut errors = 0u64;
    // Which way each change went, because "5 rows changed" says nothing
    // about whether the change was the one intended.
    let mut transitions: BTreeMap<String, u64> = BTreeMap::new();
    let mut samples: Vec<serde_json::Value> = Vec::new();

    for user in &users {
        // (folder, base id) -> path, from one pass over this mailbox.
        //
        // The folder is part of the key because a `blob_ref` names one —
        // `.Junk/xyz` — and a map keyed on the base alone would answer
        // with whichever folder happened to be read last. Keyed this way
        // the lookup asks the same question `Maildir::locate` does.
        //
        // It changed nothing when it was added, which is worth recording:
        // it was written to explain 19 rows whose stored verdict differs
        // from their file's, and the count was identical afterwards. The
        // 19 are something else — see the shadow's own report.
        let mut disk: HashMap<(String, String), std::path::PathBuf> = HashMap::new();
        for mb in crate::imap::backend::list_mailboxes(&state, user) {
            let folder = folder_of(&mb.path);
            for sub in ["new", "cur"] {
                let Ok(rd) = std::fs::read_dir(std::path::Path::new(&mb.path).join(sub)) else {
                    continue;
                };
                for e in rd.flatten() {
                    let path = e.path();
                    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                        continue;
                    };
                    let base = name.split(':').next().unwrap_or(name).to_string();
                    disk.insert((folder.clone(), base), path);
                }
            }
        }

        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            for mid in state
                .mailbox
                .user_thread_message_ids(user, &tid)
                .unwrap_or_default()
            {
                // The overlaid view, not the shared blob: since stage 5
                // the shared row carries no per-user fields, so reading it
                // directly and writing it back would blank this user's
                // `blob_ref`, `uid` and flags.
                let Ok(Some(wire_bytes)) = state.mailbox.user_message_view(user, &mid) else {
                    continue;
                };
                let Ok(mut wire) = serde_json::from_slice::<
                    mailrs_core_api::method::message::MessageWire,
                >(&wire_bytes) else {
                    errors += 1;
                    continue;
                };
                if wire.blob_ref.is_empty() {
                    no_file += 1;
                    continue;
                }
                // The blob_ref may name a subfolder (`.Junk/xyz`) and may
                // carry a `:2,FLAGS` suffix. Split both off: the folder is
                // half the key, and the flag-free base is the half that
                // survives a rename.
                let (folder, leaf) = match wire.blob_ref.rsplit_once('/') {
                    Some((f, l)) if f.starts_with('.') => (f.to_string(), l),
                    _ => (String::new(), wire.blob_ref.as_str()),
                };
                let base = leaf.split(':').next().unwrap_or(leaf).to_string();
                let Some(path) = disk.get(&(folder, base)) else {
                    no_file += 1;
                    continue;
                };
                let Ok(raw) = std::fs::read(path) else {
                    no_file += 1;
                    continue;
                };
                walked += 1;

                let fresh = crate::extract_sender_trust(&raw);
                if fresh == wire.sender_trust {
                    agreed += 1;
                    continue;
                }
                if !overwrites(&wire.sender_trust, &fresh) {
                    silent += 1;
                    continue;
                }
                *transitions
                    .entry(format!(
                        "{} -> {}",
                        blank_as_none(&wire.sender_trust),
                        blank_as_none(&fresh)
                    ))
                    .or_default() += 1;
                if samples.len() < 20 {
                    samples.push(serde_json::json!({
                        "user": user,
                        "blob_ref": wire.blob_ref,
                        "path": path.display().to_string(),
                        "sender": wire.sender,
                        "stored": wire.sender_trust,
                        "recomputed": fresh,
                    }));
                }
                changed += 1;
                if q.dry_run {
                    continue;
                }
                wire.sender_trust = fresh;
                let Ok(payload) = serde_json::to_vec(&wire) else {
                    errors += 1;
                    continue;
                };
                // Hand back this user's own facts explicitly. The write
                // path takes a zero or a blank as "I have nothing to say
                // about this field" and keeps what is stored, so passing
                // the read-back values through is safe as well as correct.
                let facts = match state.mailbox.user_message_facts(user, &mid) {
                    Ok(Some(f)) => f,
                    _ => {
                        errors += 1;
                        continue;
                    }
                };
                if let Err(e) = state.mailbox.upsert_user_message(
                    user,
                    &tid,
                    &wire.message_id,
                    wire.internal_date,
                    &payload,
                    &mailrs_mailbox_kevy::UserMessageFacts {
                        blob_ref: &facts.blob_ref,
                        uid: facts.uid,
                        flags: facts.flags,
                        modseq: facts.modseq,
                    },
                ) {
                    tracing::warn!(err = %e, %user, %mid, "recompute-sender-trust: write failed");
                    errors += 1;
                }
            }
        }
    }

    axum::Json(serde_json::json!({
        "dry_run": q.dry_run,
        "users": users.len(),
        "copies_walked": walked,
        "agreed": agreed,
        "changed": changed,
        "no_file": no_file,
        "silent_copy": silent,
        "errors": errors,
        "transitions": transitions,
        "samples": samples,
        "store_motion": motion.finish(&state),
    }))
    .into_response()
}

/// Whether a freshly-derived verdict may replace the stored one.
///
/// **An empty verdict is not a verdict.** `extract_sender_trust` says so
/// in its own words: empty means *nothing to say* — no
/// `Authentication-Results` header and no tampering found — and never
/// that the message was examined and found safe. So a copy that has
/// nothing to say must not overwrite a copy that had something.
///
/// This is not a nicety, it is what makes the route terminate. The
/// verdict lives on the **shared** message blob while the file it is
/// derived from is **per user**, so a message two people hold can have a
/// stamped copy and an unstamped one. Without this rule the two owners
/// overwrite each other forever: measured on a copy of production, one
/// pass reported 19 `verified -> (none)` *and* 19 `(none) -> verified`,
/// and the next pass had the same 19 waiting. A repair that never
/// reaches "nothing to do" is not a repair.
fn overwrites(stored: &str, fresh: &str) -> bool {
    !(fresh.is_empty() && !stored.is_empty())
}

/// Which folder a mailbox path names, as a `blob_ref` would spell it:
/// the `.Junk`-style leaf for a subfolder, empty for the root INBOX.
fn folder_of(path: &std::path::Path) -> String {
    match path.file_name().and_then(|n| n.to_str()) {
        Some(leaf) if leaf.starts_with('.') => leaf.to_string(),
        _ => String::new(),
    }
}

/// An empty verdict means *nothing to say*, and printing it as `""` in a
/// transition key reads as a missing value rather than a stated one.
fn blank_as_none(v: &str) -> &str {
    if v.is_empty() { "(none)" } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The convergence property, stated as the rule rather than as a run:
    /// nothing a silent copy can say makes a recorded verdict move.
    #[test]
    fn a_silent_copy_never_overwrites_a_recorded_verdict() {
        assert!(!overwrites("verified", ""));
        assert!(!overwrites("suspicious", ""));
        assert!(!overwrites("unverified", ""));
    }

    /// And a real verdict replaces a real verdict — otherwise the route
    /// could converge by doing nothing at all, which is the failure this
    /// rule is one step away from.
    #[test]
    fn one_verdict_still_replaces_another() {
        assert!(overwrites("verified", "suspicious"));
        assert!(overwrites("unverified", "verified"));
        assert!(overwrites("", "suspicious"));
        assert!(overwrites("", "verified"));
    }

    /// A `blob_ref` is split the same way `Maildir::locate` reads it:
    /// a leading `.folder/`, and a `:2,FLAGS` suffix that a rename
    /// changes.
    #[test]
    fn folder_names_come_off_the_mailbox_path() {
        assert_eq!(
            folder_of(std::path::Path::new("/data/maildir/x/y/.Junk")),
            ".Junk"
        );
        assert_eq!(folder_of(std::path::Path::new("/data/maildir/x/y")), "");
    }

    #[test]
    fn an_absent_verdict_prints_as_a_word_not_as_empty() {
        assert_eq!(blank_as_none(""), "(none)");
        assert_eq!(blank_as_none("verified"), "verified");
    }
}
