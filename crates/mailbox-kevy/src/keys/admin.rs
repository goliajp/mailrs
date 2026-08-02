//! Keys for the admin entities and the outbound queue.

//! KV key helpers — every key the kevy backend reads or writes.
//!
//! Single source of truth. Per-method implementations call these instead
//! of writing literal `format!` strings so renames stay local.

/// Account hash — one per user address. Fields mirror
/// `AccountWithHashWire` (blob) + the range-indexed
/// `{domain, active, created_at}` triple. Enumerated via
/// `accounts_by_active` (see below) — no separate set index.
pub fn account(address: &str) -> String {
    format!("mailrs:account:{address}")
}

/// Effective permissions blob for a user — cached so login doesn't
/// need to re-compute the graph on every request.
pub fn account_permissions(address: &str) -> String {
    format!("mailrs:account:{address}:perms")
}

// ── v2.6.0 §P6 dual-write keyspace ────────────────────────────────────
//
// The legacy admin-CRUD store pattern is `mailrs:{alias,domain}:<x>`
// string + `mailrs:{aliases,domains}:index` set; listing walks the set
// and issues per-key GETs (N+1 RTT). See RFC
// `20260709-v2.3-p6-admin-crud-idx-query.md` §1.
//
// Phase 9 (this commit) introduces a parallel `v2:` hash keyspace that
// the roadmap Phase 10 will switch reads to via `idx_query_range`, and
// Phase 11 will drop the legacy prefix. Write paths dual-populate both
// keyspaces; read paths still hit the legacy layout.
//
// `mailrs:account:<addr>` is already a hash (blob field), so account
// dual-write extends the SAME key with `{domain, active, created_at}`
// derived fields — no `v2:` sibling needed.

/// v2 alias hash key: `mailrs:alias:v2:<address>` — hash
/// `{target, domain, created_at, active}`.
pub fn alias_v2(address: &str) -> String {
    format!("mailrs:alias:v2:{address}")
}

/// v2 alias index prefix used by the RANGE indexes below.
pub const ALIAS_V2_PREFIX: &[u8] = b"mailrs:alias:v2:";

/// Range index over `mailrs:alias:v2:*`.domain — one RTT list-by-domain.
pub const IDX_ALIASES_BY_DOMAIN: &[u8] = b"aliases_by_domain";

/// Range index over `mailrs:alias:v2:*`.target — reverse lookup
/// (RFC §3.1: "who forwards TO this address?").
pub const IDX_ALIASES_BY_TARGET: &[u8] = b"aliases_by_target";

/// v2 domain hash key: `mailrs:domain:v2:<name>` — hash `{created_at}`.
pub fn domain_v2(name: &str) -> String {
    format!("mailrs:domain:v2:{name}")
}

/// v2 domain index prefix.
pub const DOMAIN_V2_PREFIX: &[u8] = b"mailrs:domain:v2:";

/// Range index over `mailrs:domain:v2:*`.created_at — one RTT list
/// sorted by insertion timestamp (server-side sort).
pub const IDX_DOMAINS_BY_CREATED: &[u8] = b"domains_by_created";

/// Account index prefix — SAME key as the legacy hash, additional fields.
pub const ACCOUNT_PREFIX: &[u8] = b"mailrs:account:";

/// Range index over `mailrs:account:*`.domain.
pub const IDX_ACCOUNTS_BY_DOMAIN: &[u8] = b"accounts_by_domain";

/// Range index over `mailrs:account:*`.active — active accounts one RTT.
pub const IDX_ACCOUNTS_BY_ACTIVE: &[u8] = b"accounts_by_active";

/// Outbound queue row (for sender split).
pub fn outbound(id: i64) -> String {
    format!("mailrs:outbound:{id}")
}

/// Outbound pending queue — sender claims with BRPOPLPUSH.
pub const OUTBOUND_PENDING: &str = "mailrs:outbound:pending";

/// Outbound inflight list — for stale recovery.
pub const OUTBOUND_INFLIGHT: &str = "mailrs:outbound:inflight";

/// Suppression set — sender consults before sending.
pub const OUTBOUND_SUPPRESSION: &str = "mailrs:outbound:suppression";

#[cfg(test)]
mod prefix_tests {
    use crate::keys::*;

    /// `all_thread_ids_for_user` enumerates with a
    /// `mailrs:threaduser:{user}:*` wildcard and strips that prefix, so any
    /// other key under it is returned as a thread id.
    ///
    /// The per-user message index was first spelled
    /// `mailrs:threaduser:{user}:{tid}:messages` and the enumeration began
    /// reporting `{tid}:messages` as a thread — the multi-owner count went
    /// from 74 to 148 the moment the backfill wrote its rows. A prefix is a
    /// namespace, and putting something else inside it is a collision even
    /// when the strings differ.
    #[test]
    fn the_per_user_message_index_is_not_under_the_threaduser_prefix() {
        let enumerated = format!("mailrs:threaduser:{}:", "u@x.com");
        assert!(
            !thread_user_messages("u@x.com", "t1").starts_with(&enumerated),
            "this key would be enumerated as a thread id"
        );
        // The membership row itself is, by design — that is what the
        // wildcard is for.
        assert!(thread_user("u@x.com", "t1").starts_with(&enumerated));
    }

    /// Same reasoning for the per-user message row.
    #[test]
    fn the_per_user_message_row_is_not_under_a_scanned_prefix() {
        let enumerated = format!("mailrs:threaduser:{}:", "u@x.com");
        assert!(!user_message("u@x.com", "<m@x>").starts_with(&enumerated));
    }
}
