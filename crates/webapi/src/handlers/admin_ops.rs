//! Operational surfaces: DNS checks, maildir reconciliation, the
//! suppression list, RBL and reputation, cache flush, spam stats.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use mailrs_core_api::method::admin as wire;

use crate::WebState;
use crate::handlers::admin::*;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

mod domain_check;
pub use domain_check::check_domain_dns;

/// POST /api/admin/webhook-subscriptions
pub async fn create_webhook(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Json(req): Json<wire::CreateWebhookRequest>,
) -> Result<Json<wire::CreateWebhookResponse>, StatusCode> {
    let address = req.account_address.clone();
    let w = with_kevy(move |c| {
        mailrs_core_sidestate::families::webhooks::create(
            c,
            mailrs_core_sidestate::families::webhooks::NewWebhook {
                account_address: req.account_address,
                url: req.url,
                event_type: req.event_type,
                filter_sender: req.filter_sender,
                filter_thread_id: req.filter_thread_id,
            },
        )
    })?;
    super::audit::record(&actor, "webhook.create", &address, &format!("id={}", w.id));
    Ok(Json(wire::CreateWebhookResponse {
        id: w.id,
        signing_secret: w.signing_secret,
    }))
}

/// GET /api/admin/accounts/{address}/webhook-subscriptions
pub async fn list_webhooks(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    axum::extract::Path(address): axum::extract::Path<String>,
) -> Result<Json<wire::WebhookListResponse>, StatusCode> {
    let items = with_kevy(move |c| mailrs_core_sidestate::families::webhooks::list(c, &address))?;
    Ok(Json(wire::WebhookListResponse { items }))
}

/// DELETE /api/admin/webhook-subscriptions/{id}
pub async fn delete_webhook(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let removed = with_kevy(move |c| mailrs_core_sidestate::families::webhooks::delete(c, id))?;
    let id_str = id.to_string();
    super::audit::record(&actor, "webhook.delete", &id_str, "");
    match removed {
        true => Ok(StatusCode::NO_CONTENT),
        // Previously 204 regardless — and since the account list it searched
        // came from a swept key, always without deleting anything.
        false => Err(StatusCode::NOT_FOUND),
    }
}

/// GET /api/admin/accounts/{address}/sieve — read the user's sieve
/// script. Sieve is stored in `sieve:<addr>` string. Empty = no script.
pub async fn get_account_sieve(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = mailrs_core_sidestate::sieve_key(&address);
    let val = with_kevy(move |c| c.get(key.as_bytes()).map_err(std::io::Error::from))?;
    Ok(Json(serde_json::json!({
        "script": val.and_then(|v| String::from_utf8(v).ok()).unwrap_or_default(),
    })))
}

#[derive(Debug, serde::Deserialize)]
pub struct SetSieveRequest {
    pub script: String,
}

pub async fn set_account_sieve(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(req): Json<SetSieveRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = mailrs_core_sidestate::sieve_key(&address);
    super::audit::record(&actor, "sieve.update", &address, "");
    with_kevy(move |c| {
        c.set(key.as_bytes(), req.script.as_bytes())?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_account_sieve(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let key = mailrs_core_sidestate::sieve_key(&address);
    with_kevy(move |c| {
        c.del(&[key.as_bytes()])?;
        Ok(())
    })?;
    super::audit::record(&actor, "sieve.delete", &address, "");
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/admin/reconcile-maildir — count the mailboxes and message
/// files under `MAILRS_MAILDIR`. Read-only, and it does not compare them
/// with fastcore's index: indexing missed files is fastcore's self-heal.
pub async fn reconcile_maildir(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let (users_scanned, messages_seen) = count_maildir(std::path::Path::new(&root));
    Ok(Json(serde_json::json!({
        "users_scanned": users_scanned,
        "messages_seen": messages_seen,
    })))
}

/// `(mailboxes, message files)` under a maildir root laid out as
/// `<domain>/<local>/{cur,new}`. Dot-directories at the top are not
/// domains: `.deleted/` holds the mailboxes of deleted accounts, `.spool/`
/// the inbound queue. Shared by the REST route and the MCP tool.
pub(crate) fn count_maildir(root: &std::path::Path) -> (u64, u64) {
    let is_dir = |e: &std::fs::DirEntry| e.file_type().map(|t| t.is_dir()).unwrap_or(false);
    let mut users = 0u64;
    let mut messages = 0u64;
    let Ok(domains) = std::fs::read_dir(root) else {
        return (0, 0);
    };
    for domain in domains.flatten() {
        if !is_dir(&domain) || domain.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(user_dirs) = std::fs::read_dir(domain.path()) else {
            continue;
        };
        for u in user_dirs.flatten().filter(is_dir) {
            users += 1;
            for sub in ["cur", "new"] {
                if let Ok(items) = std::fs::read_dir(u.path().join(sub)) {
                    messages += items.count() as u64;
                }
            }
        }
    }
    (users, messages)
}

/// GET /api/admin/suppressions — every address outbound mail is held
/// back from, with what put it there (`hard_bounce` expires on its own,
/// `complaint` stays until removed here).
pub async fn list_suppressions(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use mailrs_core_sidestate::families::suppression;
    let entries = with_kevy(suppression::list)?;
    let items: Vec<serde_json::Value> = entries
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "email": e.email,
                "source": e.source,
                "reason": e.reason,
                "added_at": e.added_at,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(serde::Deserialize)]
pub struct ClearSuppressions {
    address: Option<String>,
}

/// DELETE /api/admin/suppressions — `?address=` takes that one address
/// off the list; without it, every address.
pub async fn clear_suppressions(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    axum::extract::Query(q): axum::extract::Query<ClearSuppressions>,
) -> Result<StatusCode, StatusCode> {
    use mailrs_core_sidestate::families::suppression;
    match q.address.filter(|a| !a.trim().is_empty()) {
        Some(address) => {
            let address = suppression::normalize(&address);
            let a = address.clone();
            let removed = with_kevy(move |c| suppression::remove(c, &a))?;
            if !removed {
                return Err(StatusCode::NOT_FOUND);
            }
            super::audit::record(&actor, "suppressions.remove", &address, "");
        }
        None => {
            let n = with_kevy(suppression::clear)?;
            super::audit::record(&actor, "suppressions.clear", "", &format!("removed={n}"));
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/admin/cache/flush-conversations — no-op in the fastcore
/// architecture (kevy is the source of truth, no separate cache).
/// Returns 204 so admin panels showing this button don't hang.
pub async fn flush_conversations_cache(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
) -> Result<StatusCode, StatusCode> {
    super::audit::record(&actor, "cache.flush_conversations", "", "no-op in fastcore");
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/admin/rbl-status — return the last RBL check result from
/// kevy `admin:rbl:status` (populated by an out-of-band worker; empty
/// object until such a worker is wired up).
pub async fn get_rbl_status(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let val = with_kevy(|c| c.get(b"admin:rbl:status").map_err(std::io::Error::from))?;
    let parsed: serde_json::Value = val
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| serde_json::json!({ "status": "unknown", "checked_at": null }));
    Ok(Json(parsed))
}

/// GET /api/admin/reputation — sender reputation snapshot from
/// `admin:reputation`. Empty until the reputation subsystem writes.
pub async fn get_reputation(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let val = with_kevy(|c| c.get(b"admin:reputation").map_err(std::io::Error::from))?;
    let parsed: serde_json::Value = val
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| serde_json::json!({ "score": null, "signals": [] }));
    Ok(Json(parsed))
}

/// GET /api/admin/spam-feedback-stats — aggregate spam-feedback hash
/// across all users. `spam_feedback:<user>` → { message_id -> label }.
/// Sum labels into { spam, ham, per_user }.
pub async fn get_spam_feedback_stats(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Same swept key as `delete_webhook` used: this returned an empty
    // `per_user` and zero totals on every call, which reads as "no feedback
    // yet" rather than "did not look".
    let accounts: Vec<Vec<u8>> = state
        .core
        .list_accounts()
        .await
        .map_err(map_err)?
        .items
        .into_iter()
        .map(|a| a.address.into_bytes())
        .collect();
    let mut spam_total = 0u64;
    let mut ham_total = 0u64;
    let mut per_user = serde_json::Map::new();
    for addr in accounts {
        let Some(addr_s) = String::from_utf8(addr).ok() else {
            continue;
        };
        let key = format!("spam_feedback:{addr_s}");
        let flat = with_kevy(move |c| c.hgetall(key.as_bytes()).map_err(std::io::Error::from))
            .unwrap_or_default();
        let mut spam = 0u64;
        let mut ham = 0u64;
        let mut i = 0;
        while i + 1 < flat.len() {
            match std::str::from_utf8(&flat[i + 1]).unwrap_or("") {
                "spam" => spam += 1,
                "ham" => ham += 1,
                _ => {}
            }
            i += 2;
        }
        spam_total += spam;
        ham_total += ham;
        per_user.insert(addr_s, serde_json::json!({ "spam": spam, "ham": ham }));
    }
    Ok(Json(serde_json::json!({
        "spam": spam_total,
        "ham": ham_total,
        "per_user": per_user,
    })))
}

#[cfg(test)]
mod tests {
    use super::count_maildir;

    #[test]
    fn a_deleted_mailbox_is_not_counted_as_a_domain() {
        let root = tempfile::tempdir().unwrap();
        let mk = |rel: &str, files: usize| {
            let dir = root.path().join(rel);
            std::fs::create_dir_all(&dir).unwrap();
            for i in 0..files {
                std::fs::write(dir.join(format!("m{i}")), "x").unwrap();
            }
        };
        mk("example.test/alice/cur", 2);
        mk("example.test/alice/new", 1);
        mk("example.test/bob/cur", 0);
        mk(".deleted/example.test/carol.1760000000/cur", 5);
        mk(".spool/incoming/new", 3);
        assert_eq!(count_maildir(root.path()), (2, 3));
    }
}
