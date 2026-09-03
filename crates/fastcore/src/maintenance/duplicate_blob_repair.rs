//! Drop the second registration of a file that has two.
//!
//! `duplicate_blob_census` established the shape and the gate it set
//! came back clean on production 2026-09-02:
//!
//! ```text
//! messages_walked  36062      duplicated_blobs   41
//! distinct_blobs   36021      with_synthetic_id  41
//! no_blob_ref          0      both_ids_real       0
//! ```
//!
//! `both_ids_real: 0` is the reason this may run at all. Every
//! duplicate pairs one real `Message-ID` against one synthetic
//! `{now}.{maildir_id}@mailrs.local`, which is minted in exactly one
//! place — `crates/server/src/smtp_session/process_delivered.rs`, in a
//! binary that left the image on 2026-07-22. Their dates stop on
//! 2026-07-02. A pair of real ids would have meant a live second
//! writer and a different problem.
//!
//! # Which row goes
//!
//! The synthetic one, and the argument is not "it looks fake":
//!
//! - its id appears in no message header anywhere, so no reply's
//!   `In-Reply-To` can ever name it and no thread can ever grow onto
//!   it — it is a conversation of one, permanently;
//! - the real row is the one threading resolves to, because
//!   `group_by_thread` reads the **file's own** `Message-ID:` header;
//! - it was written second in every sampled pair (its uid is the
//!   higher of the two, always), so the earlier registration is the
//!   one a reader has had longest.
//!
//! # What it will not do
//!
//! **Never unlinks the file.** `delete_thread` hands back the
//! `blob_ref`s precisely so a caller can remove them, and here that
//! would delete the surviving row's body.
//!
//! **Never touches a group it does not fully understand.** Anything
//! other than "exactly two rows, exactly one synthetic, and the
//! synthetic alone in its thread" is counted and skipped, with the
//! reason. A repair that guesses at the awkward cases is how a
//! measurement becomes damage.
//!
//! # Measured on a copy of production, 2026-09-03
//!
//! ```text
//! duplicated_blobs   41
//!   droppable        28   deleted, and still gone after a restart
//!   shares_a_thread  10   both rows under one thread id
//!   holds_more        3   the synthetic row's thread grew other messages
//! ```
//!
//! Afterwards the census read 13, and `messages_walked` fell 36062 →
//! 36034 — exactly the twenty-eight rows, nothing beside them.
//!
//! **The sweep was proved live before that was believed.** Nothing
//! logs when `healed_from_maildir` finds nothing to do, so "the row
//! did not come back" and "the sweep never ran" read identically. A
//! new file with a fresh `Message-ID` was dropped into the copy's
//! maildir and the restart turned it into a row — `messages_walked`
//! 36034 → 36035. The sweep can resurrect; it does not resurrect
//! these, because `group_by_thread` reads the file's own header and
//! buckets it under the surviving thread.
//!
//! The thirteen it refuses need a message-level delete, which does not
//! exist: `delete_thread` is the only removal the store has, and it
//! takes the whole conversation.

use std::collections::HashMap;

use super::prelude::*;

const PAUSE_EVERY: u64 = 400;

#[derive(serde::Deserialize)]
pub(crate) struct RepairQuery {
    /// Default DRY. Writing to a mail store is not a default.
    #[serde(default)]
    apply: bool,
    #[serde(default = "default_limit")]
    limit: u64,
    #[serde(default = "default_pause_ms")]
    pause_ms: u64,
}

fn default_limit() -> u64 {
    200_000
}
fn default_pause_ms() -> u64 {
    20
}

struct Claim {
    message_id: String,
    thread_id: String,
    uid: u32,
}

fn is_synthetic(message_id: &str) -> bool {
    message_id.ends_with("@mailrs.local")
}

/// What a duplicate group resolves to.
enum Verdict {
    /// The synthetic row, alone in its own thread. Safe to drop.
    /// Carries the uid because that is what vanishes from an IMAP
    /// client's map, and an operator reading a dry run should be able
    /// to see which one.
    DropThread {
        user: String,
        thread_id: String,
        uid: u32,
    },
    /// More than two rows claim the file.
    NotAPair,
    /// Both real, or both synthetic. The census gate says the first
    /// does not occur on this deployment; if it ever does, it is a
    /// second writer and not this route's business.
    NotOneSynthetic,
    /// Both rows sit under one thread id, so the reader sees the same
    /// message twice *inside one conversation*. Deleting the thread
    /// would take the survivor with it; this needs a message-level
    /// delete, which does not exist yet.
    SharesAThread,
    /// The synthetic row is alone in its thread id, but that thread
    /// holds other messages too — somebody else's conversation grew
    /// onto it. Counted apart from `SharesAThread` because they are
    /// different faults and a merged number would hide whichever is
    /// rarer.
    ThreadHoldsMore,
}

fn verdict(user: &str, rows: &[Claim], thread_sizes: &HashMap<String, u64>) -> Verdict {
    if rows.len() != 2 {
        return Verdict::NotAPair;
    }
    let synth: Vec<&Claim> = rows
        .iter()
        .filter(|c| is_synthetic(&c.message_id))
        .collect();
    if synth.len() != 1 {
        return Verdict::NotOneSynthetic;
    }
    let s = synth[0];
    let other = rows.iter().find(|c| !is_synthetic(&c.message_id)).unwrap();
    if s.thread_id == other.thread_id {
        return Verdict::SharesAThread;
    }
    // A thread holding more than the synthetic message is somebody
    // else's conversation; dropping it would take real mail.
    if thread_sizes.get(&s.thread_id).copied().unwrap_or(0) != 1 {
        return Verdict::ThreadHoldsMore;
    }
    Verdict::DropThread {
        user: user.to_string(),
        thread_id: s.thread_id.clone(),
        uid: s.uid,
    }
}

/// `POST /v1/admin/maintenance:duplicate-blob-repair`
pub(crate) async fn duplicate_blob_repair_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RepairQuery>,
) -> axum::response::Response {
    let users = state.mailbox.list_account_addresses().unwrap_or_default();

    let mut walked = 0u64;
    let mut claims: HashMap<(String, String), Vec<Claim>> = HashMap::new();
    let mut thread_sizes: HashMap<String, u64> = HashMap::new();

    'walk: for user in &users {
        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            let mids = state
                .mailbox
                .user_thread_message_ids(user, &tid)
                .unwrap_or_default();
            thread_sizes.insert(tid.clone(), mids.len() as u64);
            for mid in mids {
                if walked >= q.limit {
                    break 'walk;
                }
                walked += 1;
                if walked.is_multiple_of(PAUSE_EVERY) {
                    tokio::time::sleep(std::time::Duration::from_millis(q.pause_ms)).await;
                }
                let Ok(Some(bytes)) = state.mailbox.user_message_view(user, &mid) else {
                    continue;
                };
                let Ok(wire) =
                    serde_json::from_slice::<mailrs_core_api::method::message::MessageWire>(&bytes)
                else {
                    continue;
                };
                if wire.blob_ref.is_empty() {
                    continue;
                }
                claims
                    .entry((user.clone(), wire.blob_ref.clone()))
                    .or_default()
                    .push(Claim {
                        message_id: wire.message_id.clone(),
                        thread_id: wire.thread_id.clone(),
                        uid: wire.uid,
                    });
            }
        }
    }

    let mut droppable: Vec<(String, String, u32)> = Vec::new();
    let mut not_a_pair = 0u64;
    let mut not_one_synthetic = 0u64;
    let mut shares_a_thread = 0u64;
    let mut thread_holds_more = 0u64;
    let mut duplicated = 0u64;
    for ((user, _blob), rows) in claims.iter() {
        if rows.len() < 2 {
            continue;
        }
        duplicated += 1;
        match verdict(user, rows, &thread_sizes) {
            Verdict::DropThread {
                user,
                thread_id,
                uid,
            } => droppable.push((user, thread_id, uid)),
            Verdict::NotAPair => not_a_pair += 1,
            Verdict::NotOneSynthetic => not_one_synthetic += 1,
            Verdict::SharesAThread => shares_a_thread += 1,
            Verdict::ThreadHoldsMore => thread_holds_more += 1,
        }
    }
    droppable.sort();

    let mut dropped = 0u64;
    let mut drop_failed = 0u64;
    if q.apply {
        for (user, tid, _uid) in &droppable {
            // The second element is the thread's `blob_ref`s, and they
            // are deliberately discarded: the surviving row points at
            // the same file.
            match state.mailbox.delete_thread(user, tid) {
                Ok((true, _blobs)) => dropped += 1,
                Ok((false, _)) => drop_failed += 1,
                Err(e) => {
                    tracing::warn!(%user, %tid, err = %e, "duplicate-blob repair: delete failed");
                    drop_failed += 1;
                }
            }
        }
    }

    tracing::info!(
        apply = q.apply,
        walked,
        duplicated,
        droppable = droppable.len(),
        dropped,
        drop_failed,
        shares_a_thread,
        thread_holds_more,
        not_a_pair,
        not_one_synthetic,
        "duplicate-blob repair"
    );
    Json(serde_json::json!({
        "apply": q.apply,
        "messages_walked": walked,
        "duplicated_blobs": duplicated,
        // What a dry run promises to do.
        "droppable": droppable.len(),
        "dropped": dropped,
        // A delete that reported "was not there" — the count exists so
        // `droppable` and `dropped` disagreeing is visible rather than
        // rounded away.
        "drop_failed": drop_failed,
        // Skipped, by reason. Each of these is a case this route
        // declines to guess at, not a case it handled.
        "skipped_shares_a_thread": shares_a_thread,
        "skipped_thread_holds_more": thread_holds_more,
        "skipped_not_a_pair": not_a_pair,
        "skipped_not_one_synthetic": not_one_synthetic,
        "sample": droppable.iter().take(10).map(|(u, t, uid)| serde_json::json!({
            "user": u, "thread_id": t, "uid_removed": uid,
        })).collect::<Vec<_>>(),
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(mid: &str, tid: &str, uid: u32) -> Claim {
        Claim {
            message_id: mid.into(),
            thread_id: tid.into(),
            uid,
        }
    }

    fn sizes(pairs: &[(&str, u64)]) -> HashMap<String, u64> {
        pairs.iter().map(|(t, n)| ((*t).into(), *n)).collect()
    }

    #[test]
    fn the_shape_production_has_is_the_one_it_drops() {
        let rows = vec![
            claim(
                "SA1PR17MB5447@outlook.com",
                "SA1PR17MB5447@outlook.com",
                12168,
            ),
            claim(
                "1781060503.M358220P1Q61@mailrs.local",
                "1781060503.M358220P1Q61@mailrs.local",
                22608,
            ),
        ];
        let s = sizes(&[
            ("SA1PR17MB5447@outlook.com", 1),
            ("1781060503.M358220P1Q61@mailrs.local", 1),
        ]);
        match verdict("u", &rows, &s) {
            Verdict::DropThread { thread_id, uid, .. } => {
                assert!(thread_id.ends_with("@mailrs.local"), "dropped the real row");
                assert_eq!(uid, 22608, "the uid reported must be the one being removed");
            }
            _ => panic!("the census's own sample must be repairable"),
        }
    }

    /// The load-bearing refusal. One sample in ten had both rows under
    /// one thread id; deleting that thread takes the surviving message
    /// with it, and the file stays on disk either way, so the loss is
    /// silent.
    #[test]
    fn a_shared_thread_is_refused() {
        let rows = vec![
            claim("real@example.com", "shared-root", 10),
            claim("x@mailrs.local", "shared-root", 99),
        ];
        assert!(matches!(
            verdict("u", &rows, &sizes(&[("shared-root", 2)])),
            Verdict::SharesAThread
        ));
    }

    /// A synthetic row whose thread has grown other messages is not a
    /// stray registration any more, whatever its id says.
    #[test]
    fn a_synthetic_thread_that_holds_more_is_refused() {
        let rows = vec![
            claim("real@example.com", "real-root", 10),
            claim("x@mailrs.local", "synth-root", 99),
        ];
        assert!(matches!(
            verdict("u", &rows, &sizes(&[("real-root", 1), ("synth-root", 4)])),
            Verdict::ThreadHoldsMore
        ));
    }

    #[test]
    fn two_real_ids_are_never_touched() {
        let rows = vec![
            claim("a@example.com", "a", 1),
            claim("b@example.com", "b", 2),
        ];
        assert!(matches!(
            verdict("u", &rows, &sizes(&[("a", 1), ("b", 1)])),
            Verdict::NotOneSynthetic
        ));
    }

    #[test]
    fn three_claimants_are_never_touched() {
        let rows = vec![
            claim("a@example.com", "a", 1),
            claim("x@mailrs.local", "x", 2),
            claim("y@mailrs.local", "y", 3),
        ];
        assert!(matches!(
            verdict("u", &rows, &sizes(&[("a", 1), ("x", 1), ("y", 1)])),
            Verdict::NotAPair
        ));
    }
}
