//! Carrying read notes back to the server, on the pass that is
//! already connected.
//!
//! Split from `external_sync.rs` at the file-size gate along the seam
//! that was already there: this is the only part of a pass that talks
//! about what happened *here*, and everything else in that file is
//! about bringing mail in.

use std::sync::Arc;

use mailrs_core_sidestate::families::external_accounts::AccountRow;
use mailrs_imap_client as imap;

use crate::FastcoreState;
use crate::external_sync::sanitise;

/// Tell the server about messages read here since the last pass.
///
/// Failures are left alone rather than propagated: a folder that has
/// gone, or a server that refuses a STORE, must not fail the sync —
/// the point of the pass is to bring mail in, and a note that cannot
/// be carried today is carried tomorrow.
///
/// A note whose folder no longer exists is the exception: it is
/// dropped, because a job that can never finish is a queue that only
/// grows. IMAP makes a STORE for a uid the server does not hold a
/// no-op rather than an error, so the ordinary "somebody deleted it at
/// the other end" case clears itself.
pub(crate) async fn carry_read_notes(
    state: &Arc<FastcoreState>,
    row: &AccountRow,
    session: &mut imap::Session,
) {
    let pending = crate::external_writeback::pending_for(state, &row.id);
    if pending.is_empty() {
        return;
    }
    let mut done: Vec<(String, u32)> = Vec::new();
    for (folder, uids) in pending {
        // The folder name was written through `sanitise`, and IMAP
        // wants the real one. `folders_to_read` has the real names, so
        // the note is matched back against them.
        let Some(real) = real_folder_name(session, &folder).await else {
            tracing::info!(account = %row.email, %folder, "dropping read notes for a folder that is gone");
            done.extend(uids.into_iter().map(|u| (folder.clone(), u)));
            continue;
        };
        if session.select(&real).await.is_err() {
            continue;
        }
        match session.store_seen(&uids).await {
            Ok(()) => {
                tracing::info!(account = %row.email, folder = %real, n = uids.len(), "marked read on the server");
                done.extend(uids.into_iter().map(|u| (folder.clone(), u)));
            }
            Err(e) => {
                tracing::warn!(account = %row.email, folder = %real, error = %e, "could not mark read on the server");
            }
        }
    }
    crate::external_writeback::clear_pending(state, &row.id, &done);
}

/// The server's own name for a folder whose sanitised form is known.
async fn real_folder_name(session: &mut imap::Session, sanitised: &str) -> Option<String> {
    let listing = session.list().await.ok()?;
    listing
        .into_iter()
        .map(|f| f.name)
        .find(|name| sanitise(name) == sanitised)
}
