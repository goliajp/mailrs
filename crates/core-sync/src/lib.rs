//! Backend-agnostic bidirectional mail-store sync.
//!
//! Both mailrs cores (fastcore/kevy and core/pg-spg) serve the identical
//! `mailrs-core-api` contract, so this tool is direction-blind: it holds
//! two [`Client`]s (`src`, `dst`), reads only via the contract, and
//! writes only via the contract. The same [`sync`] runs PG→kevy and
//! kevy→PG — nothing here knows which backend is which.
//!
//! What moves: the switchable mail store — accounts, aliases, and every
//! per-user thread's messages (rebuilt via `deliver_message`, which
//! re-derives thread aggregates + uid index on the destination).
//!
//! What does NOT move (by design): the independent network-kevy
//! side-state (sessions, greylist, sieve, contacts, queue, drafts,
//! signatures, …), the meili index (rebuilt via backfill), the maildir
//! bodies (physically shared — `blob_ref` resolves to the same files on
//! both cores), and uid identity/modseq (each core allocates its own;
//! only per-mailbox monotonicity is preserved).
//!
//! Idempotency: `deliver_message` is Message-ID-keyed on both cores, but
//! kevy's thread counters are not idempotent, so [`sync`] dedupes on the
//! client side — it reads the destination thread's existing message-id
//! set once per thread and only delivers what's missing. Re-running is a
//! no-op.

use std::collections::HashSet;

use mailrs_core_api::client::Client;
use mailrs_core_api::error::CoreApiError;
use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::method::thread::DeliverMessageRequest;
use mailrs_core_api::types::ConversationFilter;

/// Tunables for a sync run.
#[derive(Debug, Clone)]
pub struct SyncOpts {
    /// Page size for `list_conversations` enumeration.
    pub page_size: u32,
    /// Also enumerate + transfer archived threads (a second filtered pass).
    pub include_archived: bool,
    /// Placeholder plaintext used for the initial `add_account` before the
    /// real hash is written via `set_account_password`. Never authenticates
    /// (immediately overwritten).
    pub account_placeholder_pw: String,
}

impl Default for SyncOpts {
    fn default() -> Self {
        Self {
            page_size: 200,
            include_archived: true,
            account_placeholder_pw: "sync-placeholder-never-used".to_string(),
        }
    }
}

/// Tallies from a completed run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub accounts: u64,
    pub aliases: u64,
    pub threads: u64,
    pub messages_delivered: u64,
    pub messages_skipped_dupe: u64,
}

/// Run a full mail-store sync `src` → `dst`. Idempotent.
pub async fn sync(src: &Client, dst: &Client, opts: &SyncOpts) -> Result<SyncReport, CoreApiError> {
    let mut report = SyncReport::default();

    // ── 1. accounts (address + display_name + password hash) ─────────
    let accounts = src.list_accounts().await?;
    for a in &accounts.items {
        sync_account(src, dst, &a.address, &a.display_name, opts).await?;
        report.accounts += 1;
    }

    // ── 2. aliases (source-keyed, backend-neutral) ───────────────────
    let aliases = src.list_local_aliases().await?;
    if let Some(items) = aliases.get("items").and_then(|v| v.as_array()) {
        for it in items {
            let (Some(source), Some(target)) = (
                it.get("source").and_then(|v| v.as_str()),
                it.get("target").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            dst.upsert_local_alias(source, target).await?;
            report.aliases += 1;
        }
    }

    // ── 3. per-user threads + messages ───────────────────────────────
    for a in &accounts.items {
        sync_user_threads(src, dst, &a.address, opts, &mut report).await?;
    }

    Ok(report)
}

/// Recreate one account on `dst`: `add_account` with a placeholder
/// password (needed because `add_account` hashes plaintext), then
/// overwrite with the real hash via `set_account_password`.
async fn sync_account(
    src: &Client,
    dst: &Client,
    address: &str,
    display_name: &str,
    opts: &SyncOpts,
) -> Result<(), CoreApiError> {
    use mailrs_core_api::method::admin::{AddAccountRequest, SetPasswordRequest};

    dst.add_account(&AddAccountRequest {
        address: address.to_string(),
        display_name: display_name.to_string(),
        password: opts.account_placeholder_pw.clone(),
    })
    .await?;

    if let Some(hash) = src
        .get_account_with_hash(address)
        .await?
        .password_hash
        .filter(|h| !h.is_empty())
    {
        dst.set_account_password(
            address,
            &SetPasswordRequest {
                password_hash: hash,
            },
        )
        .await?;
    }
    Ok(())
}

/// Enumerate a user's threads (paginated by `before_ts`) and deliver each
/// thread's missing messages into `dst`.
async fn sync_user_threads(
    src: &Client,
    dst: &Client,
    user: &str,
    opts: &SyncOpts,
    report: &mut SyncReport,
) -> Result<(), CoreApiError> {
    let mut seen_threads: HashSet<String> = HashSet::new();

    for archived in [false, true] {
        if archived && !opts.include_archived {
            break;
        }
        let mut before_ts: Option<i64> = None;
        loop {
            let req = ListConversationsRequest {
                filter: ConversationFilter {
                    limit: opts.page_size,
                    before_ts,
                    category: None,
                    domains: None,
                    archived,
                    ..Default::default()
                },
            };
            let page = src.list_conversations(user, &req).await?;
            if page.items.is_empty() {
                break;
            }
            let mut min_ts = i64::MAX;
            for summary in &page.items {
                min_ts = min_ts.min(summary.last_date);
                if !seen_threads.insert(summary.thread_id.clone()) {
                    continue; // already handled (tie across pages / both passes)
                }
                sync_thread(
                    src,
                    dst,
                    user,
                    &summary.thread_id,
                    &summary.category,
                    report,
                )
                .await?;
                report.threads += 1;
            }
            if page.items.len() < opts.page_size as usize {
                break;
            }
            // advance the cursor; `before_ts` is strict-less-than on both cores
            before_ts = Some(min_ts);
        }
    }
    Ok(())
}

/// Deliver every message of one thread that the destination is missing.
async fn sync_thread(
    src: &Client,
    dst: &Client,
    user: &str,
    thread_id: &str,
    category: &str,
    report: &mut SyncReport,
) -> Result<(), CoreApiError> {
    let src_msgs = src.list_thread_messages(user, thread_id).await?;

    // client-side dedup keyed on blob_ref — the maildir filename is the
    // physically-unique per-message key the store dedupes on
    // (index_message's `(mailbox, maildir_id)`). message_id is NOT
    // reliable: real mail has duplicate Message-IDs and empties, which
    // made a message_id-keyed set collapse rows and re-deliver them on
    // re-run (harmless — the store's uniqueness backstop absorbed it —
    // but it inflated the delivered count). Seed from the destination and
    // update in-loop so intra-run duplicates are caught too.
    let mut existing: HashSet<String> = match dst.list_thread_messages(user, thread_id).await {
        Ok(r) => r.items.iter().map(|m| m.blob_ref.clone()).collect(),
        Err(_) => HashSet::new(), // thread not present yet on dst
    };

    for wire in &src_msgs.items {
        if !existing.insert(wire.blob_ref.clone()) {
            report.messages_skipped_dupe += 1;
            continue;
        }
        let unread = !senders_csv_contains_user(&wire.sender, user);
        let payload_wire_json = serde_json::to_string(wire)
            .map_err(|e| CoreApiError::Internal(format!("serialize wire: {e}")))?;
        let req = DeliverMessageRequest {
            message_id: wire.message_id.clone(),
            subject: wire.subject.clone(),
            senders_csv: wire.sender.clone(),
            latest_date: wire.internal_date,
            latest_preview: String::new(),
            category: if category.is_empty() {
                "inbox".to_string()
            } else {
                category.to_string()
            },
            unread,
            uid: wire.uid,
            payload_wire_json,
        };
        dst.deliver_message(user, thread_id, &req).await?;
        report.messages_delivered += 1;
    }
    Ok(())
}

/// Whether the user's own address appears in the comma-joined sender list
/// (mirrors kevy's `senders_csv_contains_user` — a message the user sent
/// is not "unread").
fn senders_csv_contains_user(senders_csv: &str, user: &str) -> bool {
    let u = user.to_lowercase();
    senders_csv
        .split(',')
        .any(|s| s.trim().to_lowercase().contains(&u))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn senders_csv_membership() {
        assert!(senders_csv_contains_user("Alice <a@x.y>, b@x.y", "a@x.y"));
        assert!(!senders_csv_contains_user("b@x.y, c@x.y", "a@x.y"));
    }

    #[test]
    fn default_opts_are_sane() {
        let o = SyncOpts::default();
        assert_eq!(o.page_size, 200);
        assert!(o.include_archived);
    }

    #[test]
    fn report_accumulates() {
        let mut r = SyncReport::default();
        r.messages_delivered += 3;
        r.messages_skipped_dupe += 1;
        assert_eq!(r.messages_delivered, 3);
        assert_eq!(r.messages_skipped_dupe, 1);
    }
}
