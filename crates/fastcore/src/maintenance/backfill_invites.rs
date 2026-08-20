//! Read the invitations out of mail that arrived before anybody looked.
//!
//! `invite_method` is computed at ingest, so the change that started
//! computing it reaches new mail immediately and reaches stored mail
//! never. Self-heal does not close the gap either: it writes rows that
//! are *missing*, not rows that are silent about a field that did not
//! exist when they were written.
//!
//! Every meeting already in every mailbox is in that state — which is
//! most of them, since production ingested invitations for a year
//! without extracting one. Without this route the feature works for the
//! next invitation and stays blank for every invitation anyone has.
//!
//! Idempotent, and safe to re-run in any order: the event store keeps
//! the highest `SEQUENCE`, so re-reading an older copy of a re-sent
//! meeting cannot walk it backwards.
//!
//! Reports what it **walked** apart from what it changed, so a second
//! run reads `changed: 0` against an unchanged `copies_walked` rather
//! than looking like it found nothing.

use std::collections::HashMap;

use super::prelude::*;
use super::recompute_sender_trust::folder_of;

#[derive(serde::Deserialize, Default)]
pub(crate) struct BackfillInvitesQuery {
    /// Report what would change without changing it.
    #[serde(default)]
    dry_run: bool,
}

/// `POST /v1/admin/maintenance:backfill-invites?dry_run=true`
pub(crate) async fn backfill_invites_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<BackfillInvitesQuery>,
) -> axum::response::Response {
    let motion = crate::store_motion::begin(&state);
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut walked = 0u64;
    let mut no_file = 0u64;
    let mut not_an_invite = 0u64;
    let mut already = 0u64;
    let mut changed = 0u64;
    let mut errors = 0u64;
    let mut by_method: std::collections::BTreeMap<String, u64> = Default::default();
    let mut samples: Vec<serde_json::Value> = Vec::new();

    for user in &users {
        // One directory pass per mailbox, keyed the way a `blob_ref`
        // names a file. Per-message `Maildir::fetch` falls back to a
        // linear scan of `cur/`, which is what made the first version of
        // the sender-trust route sit at 100% of a core for eleven
        // minutes.
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

                let Some(found) = crate::invites::find(&raw) else {
                    not_an_invite += 1;
                    continue;
                };
                *by_method.entry(found.method.clone()).or_default() += 1;
                if wire.invite_method == found.method {
                    already += 1;
                    continue;
                }
                if samples.len() < 20 {
                    samples.push(serde_json::json!({
                        "user": user,
                        "subject": wire.subject,
                        "method": found.method,
                        "sequence": found.sequence,
                    }));
                }
                changed += 1;
                if q.dry_run {
                    continue;
                }
                crate::invites::store(&state, &wire.message_id, &found);
                crate::invites::file_event(&state, user, &found);
                wire.invite_method = found.method.clone();
                let Ok(payload) = serde_json::to_vec(&wire) else {
                    errors += 1;
                    continue;
                };
                // This user's own facts, handed back explicitly: the
                // write path reads a blank or a zero as "nothing to say"
                // and keeps what is stored, so passing the read-back
                // values through is both safe and required.
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
                    tracing::warn!(err = %e, %user, %mid, "backfill-invites: write failed");
                    errors += 1;
                }
            }
        }
    }

    axum::Json(serde_json::json!({
        "dry_run": q.dry_run,
        "users": users.len(),
        // What it looked at, so a run that changes nothing can be told
        // apart from a run that read nothing.
        "copies_walked": walked,
        "carried_an_invitation": by_method.values().sum::<u64>(),
        "by_method": by_method,
        "already_recorded": already,
        "changed": changed,
        "not_an_invitation": not_an_invite,
        "no_file": no_file,
        "errors": errors,
        "samples": samples,
        "store": motion.finish(&state),
    }))
    .into_response()
}
