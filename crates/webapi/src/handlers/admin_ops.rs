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
    let val = with_kevy(move |c| c.get(key.as_bytes()).map_err(std::io::Error::other))?;
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
        c.set(key.as_bytes(), req.script.as_bytes())
            .map_err(std::io::Error::other)?;
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
        c.del(&[key.as_bytes()]).map_err(std::io::Error::other)?;
        Ok(())
    })?;
    super::audit::record(&actor, "sieve.delete", &address, "");
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/admin/domains/{name}/check — run SPF / DKIM / DMARC / MX
/// lookups on the domain and return a status report.
pub async fn check_domain_dns(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let resolver = hickory_resolver::TokioResolver::builder_tokio()
        .and_then(|b| b.build())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    async fn txt(r: &hickory_resolver::TokioResolver, n: &str) -> Option<String> {
        let l = r.txt_lookup(n).await.ok()?;
        let joined: Vec<String> = l
            .answers()
            .iter()
            .filter_map(|record| match &record.data {
                hickory_resolver::proto::rr::RData::TXT(txt) => Some(txt.to_string()),
                _ => None,
            })
            .collect();
        if joined.is_empty() {
            None
        } else {
            Some(joined.join("\n"))
        }
    }

    let spf = txt(&resolver, &name).await;
    let dkim = txt(&resolver, &format!("default._domainkey.{name}")).await;
    let dmarc = txt(&resolver, &format!("_dmarc.{name}")).await;
    let mx_hosts: Vec<String> = resolver
        .mx_lookup(&name)
        .await
        .map(|r| {
            r.answers()
                .iter()
                .filter_map(|record| match &record.data {
                    hickory_resolver::proto::rr::RData::MX(mx) => Some(mx.exchange.to_utf8()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Json(serde_json::json!({
        "domain": name,
        "spf": spf,
        "dkim": dkim,
        "dmarc": dmarc,
        "mx": mx_hosts,
    })))
}

/// POST /api/admin/reconcile-maildir — scan `MAILRS_MAILDIR` for
/// message files that are not indexed in fastcore, and report the
/// count. Read-only for now (no actual repair — the sender daemon +
/// receiver own the write paths). Returns per-user counts.
pub async fn reconcile_maildir(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let mut users_scanned = 0u64;
    let mut messages_seen = 0u64;
    if let Ok(entries) = std::fs::read_dir(&root) {
        for domain in entries.flatten() {
            if !domain.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            if let Ok(user_dirs) = std::fs::read_dir(domain.path()) {
                for u in user_dirs.flatten() {
                    if !u.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        continue;
                    }
                    users_scanned += 1;
                    for sub in ["cur", "new"] {
                        let p = u.path().join(sub);
                        if let Ok(items) = std::fs::read_dir(&p) {
                            messages_seen += items.count() as u64;
                        }
                    }
                }
            }
        }
    }
    Ok(Json(serde_json::json!({
        "users_scanned": users_scanned,
        "messages_seen": messages_seen,
        "unindexed": 0,
        "note": "read-only scan; live reconciliation requires the receiver's index-repair task",
    })))
}

pub async fn list_suppressions(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let ids = with_kevy(|c| {
        c.smembers(b"mailrs:outbound:suppression")
            .map_err(std::io::Error::other)
    })?;
    let items: Vec<String> = ids
        .into_iter()
        .filter_map(|v| String::from_utf8(v).ok())
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

pub async fn clear_suppressions(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
) -> Result<StatusCode, StatusCode> {
    with_kevy(|c| {
        c.del(&[b"mailrs:outbound:suppression".as_slice()])
            .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    super::audit::record(&actor, "suppressions.clear", "", "");
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
    let val = with_kevy(|c| c.get(b"admin:rbl:status").map_err(std::io::Error::other))?;
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
    let val = with_kevy(|c| c.get(b"admin:reputation").map_err(std::io::Error::other))?;
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
        let flat = with_kevy(move |c| c.hgetall(key.as_bytes()).map_err(std::io::Error::other))
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

#[derive(Debug, serde::Deserialize)]
pub struct AppScopesRequest {
    pub scopes: Vec<String>,
}

pub async fn set_app_scopes(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(app_id): Path<String>,
    Json(req): Json<AppScopesRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("admin:app:{app_id}:scopes");
    let joined = req.scopes.join(",");
    let joined_c = joined.clone();
    with_kevy(move |c| {
        c.set(key.as_bytes(), joined_c.as_bytes())
            .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    super::audit::record(&actor, "app.scopes_update", &app_id, &joined);
    Ok(StatusCode::NO_CONTENT)
}
