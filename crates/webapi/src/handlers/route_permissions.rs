//! What each admin route needs, declared once and enforced by
//! `kevy_util::admin_middleware` from the route's template. A route not
//! listed needs a super administrator: a new route is refused to everyone
//! else until someone decides who may call it.
//!
//! Super administrators pass every check. The routes kept for them alone
//! show other people's mail traffic, what they read or how they sign in,
//! write someone's sieve script, lock people out, or hold someone's mail.

/// What a route asks of the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// This permission, or a super administrator.
    Permission(&'static str),
    /// Any `admin.*` permission: views that show only counts.
    AnyAdmin,
    /// A super administrator.
    Super,
}

use Need::{AnyAdmin, Permission, Super};

const ACCOUNTS: Need = Permission("admin.accounts");
const ALIASES: Need = Permission("admin.aliases");
const DOMAINS: Need = Permission("admin.domains");
const GROUPS: Need = Permission("admin.groups");
const GREYLIST: Need = Permission("admin.greylist");
const SYSTEM: Need = Permission("admin.system_config");
const CACHE: Need = Permission("admin.cache");
const OAUTH: Need = Permission("admin.oauth_clients");

/// Every `/api/admin` route template and what it needs. Kept sorted.
pub const ADMIN_ROUTES: &[(&str, Need)] = &[
    ("/api/admin/accounts", ACCOUNTS),
    ("/api/admin/accounts/{address}", ACCOUNTS),
    ("/api/admin/accounts/{address}/agent-keys", ACCOUNTS),
    ("/api/admin/accounts/{address}/forwards", ACCOUNTS),
    ("/api/admin/accounts/{address}/groups", ACCOUNTS),
    ("/api/admin/accounts/{address}/overrides", ACCOUNTS),
    ("/api/admin/accounts/{address}/password", ACCOUNTS),
    ("/api/admin/accounts/{address}/quota", ACCOUNTS),
    ("/api/admin/accounts/{address}/send-as", ACCOUNTS),
    ("/api/admin/accounts/{address}/sieve", Super),
    (
        "/api/admin/accounts/{address}/webhook-subscriptions",
        ACCOUNTS,
    ),
    ("/api/admin/aliases", ALIASES),
    ("/api/admin/aliases/{id}", ALIASES),
    ("/api/admin/audit-log", Super),
    ("/api/admin/audit-log/export", Super),
    ("/api/admin/audit/accounts", Super),
    ("/api/admin/audit/conversations", Super),
    ("/api/admin/audit/conversations/{thread_id}", Super),
    ("/api/admin/audit/conversations/{thread_id}/messages", Super),
    ("/api/admin/audit/messages/{uid}/raw", Super),
    ("/api/admin/auth-events", Super),
    ("/api/admin/cache/flush-conversations", CACHE),
    ("/api/admin/dmarc/reports", DOMAINS),
    ("/api/admin/dmarc/sources", DOMAINS),
    ("/api/admin/domains", DOMAINS),
    ("/api/admin/domains/{name}", DOMAINS),
    ("/api/admin/domains/{name}/check", DOMAINS),
    ("/api/admin/email-groups", ACCOUNTS),
    ("/api/admin/email-groups/{id}", ACCOUNTS),
    ("/api/admin/email-groups/{id}/members", ACCOUNTS),
    ("/api/admin/email-groups/{id}/members/{address}", ACCOUNTS),
    ("/api/admin/export", Super),
    ("/api/admin/flow", AnyAdmin),
    ("/api/admin/greylist/local-lists", GREYLIST),
    ("/api/admin/greylist/local-lists/{id}", GREYLIST),
    ("/api/admin/groups", GROUPS),
    ("/api/admin/groups/{id}", GROUPS),
    ("/api/admin/groups/{id}/members", GROUPS),
    ("/api/admin/groups/{id}/members/{address}", GROUPS),
    ("/api/admin/groups/{id}/permissions", GROUPS),
    ("/api/admin/mock", SYSTEM),
    ("/api/admin/mock/settings", SYSTEM),
    ("/api/admin/oauth-clients", OAUTH),
    ("/api/admin/oauth-clients/{client_id}", OAUTH),
    ("/api/admin/outbound/paused", Super),
    ("/api/admin/outbound/paused/{address}/resume", Super),
    ("/api/admin/password-clear", Super),
    ("/api/admin/password-clear/preview", Super),
    ("/api/admin/permissions", GROUPS),
    ("/api/admin/queues", Super),
    ("/api/admin/rbl-status", DOMAINS),
    ("/api/admin/reconcile-maildir", CACHE),
    ("/api/admin/reputation", DOMAINS),
    ("/api/admin/spam-feedback-stats", Super),
    ("/api/admin/suppressions", Super),
    ("/api/admin/system-config", SYSTEM),
    ("/api/admin/system-config/{key}", SYSTEM),
    ("/api/admin/webhook-subscriptions", ACCOUNTS),
    ("/api/admin/webhook-subscriptions/{id}", ACCOUNTS),
];

/// What the route with this template needs; `Super` for one not listed.
pub fn need(template: &str) -> Need {
    ADMIN_ROUTES
        .binary_search_by(|(t, _)| t.cmp(&template))
        .map(|i| ADMIN_ROUTES[i].1)
        .unwrap_or(Super)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_sorted_so_lookups_find_every_row() {
        assert!(
            ADMIN_ROUTES.windows(2).all(|w| w[0].0 < w[1].0),
            "keep ADMIN_ROUTES sorted and free of duplicates"
        );
        assert_eq!(need("/api/admin/domains"), DOMAINS);
        assert_eq!(need("/api/admin/no-such-route"), Super);
    }
}
