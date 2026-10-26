//! The text, range and aggregate indexes declared at boot.

use super::KevyMailboxStore;
use super::keys;

impl KevyMailboxStore {
    pub fn ensure_admin_indexes(&self) {
        use kevy_embedded::{IndexKind, IndexValType};
        let s = &self.store;
        // Full-text over the synthesised `search_blob` field. kevy
        // maintains this from its commit hook, so it cannot drift from
        // the rows the way an external search service does — which is
        // exactly how conversation search ended up dead for weeks
        // (index name mismatch + dropped writes, 2026-07-19).
        // Dictionary-free CJK bigrams, so Japanese subjects and sender
        // names are searchable without an analyzer.
        let _ = s.idx_create(
            keys::IDX_THREAD_SEARCH,
            keys::THREAD_PREFIX,
            keys::THREAD_SEARCH_FIELD,
            IndexValType::Str,
            IndexKind::Text,
        );
        // Message bodies, indexed separately from the thread rows so a
        // long conversation doesn't rewrite one ever-growing value on
        // every arrival.
        let _ = s.idx_create(
            keys::IDX_MESSAGE_TEXT,
            keys::MSGTEXT_PREFIX,
            keys::MESSAGE_TEXT_FIELD,
            IndexValType::Str,
            IndexKind::Text,
        );
        // Text extracted from PDF attachments, one row per attachment,
        // written by the fastcore worker after delivery.
        let _ = s.idx_create(
            keys::IDX_ATTACHMENT_TEXT,
            keys::ATTTEXT_PREFIX,
            keys::ATTACHMENT_TEXT_FIELD,
            IndexValType::Str,
            IndexKind::Text,
        );
        // Per-thread message counts, kept by the engine.
        //
        // `count`, `unread_count` and `sent_count` were three numbers
        // written by hand onto two rows, and every repair path in this
        // crate that was not about mail — `recount-threads`,
        // `shadow-counts`, `repair_thread_counts` and its four variants,
        // reindex's counts leg — existed because they drift. A count the
        // engine derives from the rows cannot, and all of that machinery
        // is deleted (C5c).
        //
        // The aggregated field is `uid` and its value is never read: `count`
        // comes free whatever it is, and `idx_create_agg` requires a numeric
        // one. `uid` is the safe choice because `upsert_user_message` writes
        // it with the other three on every row it has ever written — a field
        // that fails to coerce puts the row in `excluded`, which is an
        // undercount that reports itself as a healthy number.
        //
        // Nothing reads this yet. See `messages/group.rs` for the group key
        // and the plan's phase C for the shadow that has to agree before any
        // reader moves onto it.
        let _ = s.idx_create_agg(
            keys::IDX_USERMSG_COUNTS,
            keys::USER_MESSAGE_PREFIX,
            b"uid",
            IndexValType::I64,
            keys::USER_MESSAGE_GROUP_FIELD,
        );
        let _ = s.idx_create(
            keys::IDX_USERMSG_BY_TID,
            keys::USER_MESSAGE_PREFIX,
            b"tid",
            IndexValType::Str,
            IndexKind::Range,
        );
        let _ = s.idx_create(
            keys::IDX_ALIASES_BY_DOMAIN,
            keys::ALIAS_V2_PREFIX,
            b"domain",
            IndexValType::Str,
            IndexKind::Range,
        );
        let _ = s.idx_create(
            keys::IDX_ALIASES_BY_TARGET,
            keys::ALIAS_V2_PREFIX,
            b"target",
            IndexValType::Str,
            IndexKind::Range,
        );
        let _ = s.idx_create(
            keys::IDX_DOMAINS_BY_CREATED,
            keys::DOMAIN_V2_PREFIX,
            b"created_at",
            IndexValType::I64,
            IndexKind::Range,
        );
        let _ = s.idx_create(
            keys::IDX_ACCOUNTS_BY_DOMAIN,
            keys::ACCOUNT_PREFIX,
            b"domain",
            IndexValType::Str,
            IndexKind::Range,
        );
        let _ = s.idx_create(
            keys::IDX_ACCOUNTS_BY_ACTIVE,
            keys::ACCOUNT_PREFIX,
            b"active",
            IndexValType::Str,
            IndexKind::Range,
        );
    }
}
