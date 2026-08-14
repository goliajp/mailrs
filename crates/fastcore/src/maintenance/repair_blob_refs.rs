//! Give back the file reference to messages that can be seen but not opened.
//!
//! `maintenance:read-state-shadow` reported 215 per-user message rows whose
//! `blob_ref` is empty. That is not the harmless state it first looked like:
//! the read path returns such a row **in full** — subject, sender, date,
//! size — with `blob_ref: ""`, so the message is listed in the mailbox and
//! its body cannot be fetched. Probed on production:
//!
//! ```text
//! {"id":13521,"uid":0,"blob_ref":"","subject":"Microsoft Partnership",
//!  "sender":"Nisha Guillemard <...>","size":12685, ...}
//! ```
//!
//! `id` is the deprecated SQL primary key and is non-zero only on rows that
//! predate fastcore, and `uid` is 0 — so these are migration leftovers that
//! lost their file reference on the way across. The samples are ordinary
//! inbound mail (codeproject, quoramail, outlook, gmail), not drafts or
//! synthetic rows.
//!
//! The bodies are very likely still on disk under a name nobody recorded,
//! and a maildir file states its own `Message-ID`, so the mapping can be
//! rebuilt by reading them. That is what this does.
//!
//! Stores the **base id**, without any `:2,FLAGS` suffix: resolution matches
//! on the base id anyway, so a ref written without the suffix cannot go
//! stale the next time the message is read and renamed.
//!
//! `no_file_found` is the counter that decides whether this is a repair or
//! a loss, and it can come out zero.

use std::collections::BTreeMap;

use mailrs_core_api::method::message::MessageWire;

use super::prelude::*;

#[derive(serde::Deserialize, Default)]
pub(crate) struct RepairQuery {
    #[serde(default)]
    dry_run: bool,
}

/// `POST /v1/admin/maintenance:repair-blob-refs?dry_run=true`
pub(crate) async fn repair_blob_refs_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RepairQuery>,
) -> axum::response::Response {
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());

    let mut walked = 0u64;
    let mut missing_ref = 0u64;
    let mut repaired = 0u64;
    let mut no_file_found = 0u64;
    let mut errors = 0u64;
    let mut by_user: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    let mut unfound: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for user in &users {
        // One pass over the user's mail, reading each file's Message-ID.
        // Expensive, which is why this is a hand-run route and not a sweep.
        let by_mid = user_files_by_message_id(&root, user);
        let mut u_repaired = 0u64;
        let mut u_unfound = 0u64;

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
                walked += 1;
                let Ok(Some(facts)) = state.mailbox.user_message_facts(user, &mid) else {
                    continue;
                };
                if !facts.blob_ref.is_empty() {
                    continue;
                }
                missing_ref += 1;

                let Some(found) = by_mid.get(&mid) else {
                    // No file on disk states this Message-ID. The body is
                    // gone, not merely unreferenced, and this route cannot
                    // invent it — the row is reported rather than touched.
                    no_file_found += 1;
                    u_unfound += 1;
                    let per_user = unfound.entry(user.clone()).or_default();
                    if per_user.len() < 4 {
                        per_user.push(mid.clone());
                    }
                    continue;
                };
                // Base id: the suffix changes every time the message is read.
                let base = match found.rsplit_once('/') {
                    Some((sub, file)) => {
                        format!("{sub}/{}", file.split(':').next().unwrap_or(file))
                    }
                    None => found.split(':').next().unwrap_or(found).to_string(),
                };

                if q.dry_run {
                    repaired += 1;
                    u_repaired += 1;
                    continue;
                }
                match write_blob_ref(
                    &state,
                    user,
                    &mid,
                    facts.uid,
                    facts.flags,
                    facts.modseq,
                    &base,
                ) {
                    Ok(()) => {
                        repaired += 1;
                        u_repaired += 1;
                    }
                    Err(e) => {
                        tracing::warn!(err = %e, %user, %mid, "repair-blob-refs: write failed");
                        errors += 1;
                    }
                }
            }
        }

        if u_repaired > 0 || u_unfound > 0 {
            by_user.insert(
                user.clone(),
                serde_json::json!({ "repaired": u_repaired, "no_file_found": u_unfound }),
            );
        }
    }

    Json(serde_json::json!({
        "dry_run": q.dry_run,
        "walked": walked,
        "missing_ref": missing_ref,
        "repaired": repaired,
        "no_file_found": no_file_found,
        "errors": errors,
        "by_user": by_user,
        "samples": { "no_file_found": unfound },
    }))
    .into_response()
}

/// Put `blob_ref` back on one per-user row, leaving every other field as it
/// was. Goes through `upsert_user_message` because that is the one writer
/// of these rows; there is no narrower setter, and adding one would be a
/// second way to write the same fact.
fn write_blob_ref(
    state: &Arc<FastcoreState>,
    user: &str,
    message_id: &str,
    uid: u32,
    flags: u32,
    modseq: u64,
    blob_ref: &str,
) -> std::io::Result<()> {
    // By message-id, not by uid. These rows carry `uid: 0` — that is one of
    // the things that identifies them as pre-fastcore leftovers — so
    // `get_message_by_uid` cannot find a single one of them and this route
    // would have reported 215 errors and zero repairs. Caught by reading
    // the code back before it ran, not by running it.
    let Some(bytes) = state.mailbox.user_message_view(user, message_id)? else {
        return Err(std::io::Error::other("no wire for row"));
    };
    let mut wire: MessageWire = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
    wire.blob_ref = blob_ref.to_string();
    let json = serde_json::to_vec(&wire).map_err(std::io::Error::other)?;
    state.mailbox.upsert_user_message(
        user,
        &wire.thread_id,
        &wire.message_id,
        wire.date,
        &json,
        &mailrs_mailbox_kevy::UserMessageFacts {
            blob_ref,
            uid,
            flags,
            modseq,
        },
    )
}
