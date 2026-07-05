//! Admin handlers — Phase 2.2 priority subset for webapi split unblock.
//!
//! Covers the auth hot-path:
//! - GET /v1/admin/api-keys/by-prefix/{prefix}     — every API-key authed req
//! - POST /v1/admin/api-keys/{id}/touch            — async update_last_used
//! - GET /v1/admin/accounts/{address}/effective-permissions  — every authed req
//! - GET /v1/admin/accounts/{address}/credentials  — SMTP/IMAP/POP3 AUTH
//! - GET /v1/admin/accounts                        — list (admin web UI)
//! - GET /v1/admin/accounts/{address}              — get one (admin web UI)
//!
//! Remaining 50+ admin endpoints land in subsequent loops.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::TimeZone;

use mailrs_core_api::method::admin as wire;

use crate::core_rpc::CoreRpcState;

// ── api keys ────────────────────────────────────────────────────────

// ── effective permissions ───────────────────────────────────────────

/// GET /v1/admin/accounts/{address}/effective-permissions
///
/// Called per authed web request (`/api/auth/me` + middleware). Critical
/// hot path — webapi caches the result in kevy session.
pub async fn effective_permissions(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<Json<wire::EffectivePermissionsResponse>, StatusCode> {
    let perms = state
        .domain
        .load_account_permissions(&address)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, address = %address, "load perms failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(wire::EffectivePermissionsResponse {
        address: address.clone(),
        permissions: perms.permission_list(),
        // Group resolution is left for a separate RPC call to keep this
        // endpoint cheap; webapi can hit /accounts/{address}/groups when it
        // needs the group list (rarely; not part of the auth-me hot path).
        groups: Vec::new(),
        is_super: perms.is_super(),
        send_as: perms.send_as().to_vec(),
    }))
}

// ── accounts (read) ─────────────────────────────────────────────────

/// GET /v1/admin/accounts/{address}/credentials
///
/// SMTP/IMAP/POP3/MgSieve AUTH path. Returns Account public fields + the
/// argon2 password hash. Bearer-secret-only; never expose to webapi public.
pub async fn get_account_with_hash(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<Json<wire::AccountWithHashWire>, StatusCode> {
    let (account, password_hash) = state
        .domain
        .get_account_with_hash(&address)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, address = %address, "get_account_with_hash failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let public = wire::AccountWire {
        address: account.address,
        domain: account.domain,
        display_name: account.display_name,
        active: account.active,
        created_at: account.created_at,
        quota_bytes: account.quota_bytes,
        recovery_email: if account.recovery_email.is_empty() {
            None
        } else {
            Some(account.recovery_email)
        },
    };
    Ok(Json(wire::AccountWithHashWire {
        public,
        password_hash: Some(password_hash),
    }))
}

/// GET /v1/admin/accounts
pub async fn list_accounts(
    State(state): State<Arc<CoreRpcState>>,
) -> Result<Json<wire::AccountListResponse>, StatusCode> {
    let rows = state.domain.list_accounts().await.map_err(|e| {
        tracing::warn!(error = %e, "list_accounts failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let items = rows
        .into_iter()
        .map(|a| wire::AccountWire {
            address: a.address,
            domain: a.domain,
            display_name: a.display_name,
            active: a.active,
            created_at: a.created_at,
            quota_bytes: a.quota_bytes,
            recovery_email: if a.recovery_email.is_empty() {
                None
            } else {
                Some(a.recovery_email)
            },
        })
        .collect();
    Ok(Json(wire::AccountListResponse { items }))
}

/// GET /v1/admin/accounts/{address}
///
/// Public account info (no hash). Caches via the same DomainStore kevy LRU.
pub async fn get_account(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<Json<wire::AccountWire>, StatusCode> {
    let (account, _) = state
        .domain
        .get_account_with_hash(&address)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, address = %address, "get_account failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(wire::AccountWire {
        address: account.address,
        domain: account.domain,
        display_name: account.display_name,
        active: account.active,
        created_at: account.created_at,
        quota_bytes: account.quota_bytes,
        recovery_email: if account.recovery_email.is_empty() {
            None
        } else {
            Some(account.recovery_email)
        },
    }))
}

// Silence unused imports under future loops:
#[allow(dead_code)]
fn _import_tz() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc.timestamp_opt(0, 0).unwrap()
}

// ── account writes (v2 dual-mode parity with fastcore) ──────────────
// The monolith did account CRUD in its web layer; in the split the PG
// core must serve these so webapi (a separate process on one core
// client) can create/update accounts regardless of backend.

/// POST /v1/admin/accounts — create/replace. Webapi sends plaintext;
/// hashed here with Argon2 so the wire mirrors fastcore's add_account.
pub async fn add_account(
    State(state): State<Arc<CoreRpcState>>,
    Json(req): Json<wire::AddAccountRequest>,
) -> Result<StatusCode, StatusCode> {
    use argon2::{
        Argon2, PasswordHasher,
        password_hash::{SaltString, rand_core::OsRng},
    };
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(req.password.as_bytes(), &salt)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();
    let domain = req.address.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
    // ensure the domain row exists so the accounts FK is satisfied —
    // domains are shared side-state (not synced), so a freshly-migrated
    // pg-core may not have them yet. Makes account creation self-sufficient.
    if !domain.is_empty() {
        let _ = sqlx::query("INSERT INTO domains (name) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(domain)
            .execute(&state.pool)
            .await;
    }
    state
        .domain
        .add_account(
            &req.address,
            domain,
            &req.display_name,
            &hash,
            chrono::Utc::now().timestamp(),
        )
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, address = %req.address, "add_account failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// PUT /v1/admin/accounts/{address} — update display name.
pub async fn update_account(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
    Json(req): Json<wire::UpdateAccountRequest>,
) -> Result<StatusCode, StatusCode> {
    state
        .domain
        .update_account_display_name(&address, &req.display_name)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /v1/admin/accounts/{address}
pub async fn remove_account(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .domain
        .remove_account(&address)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /v1/admin/accounts/{address}/quota
pub async fn set_quota(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
    Json(req): Json<wire::SetQuotaRequest>,
) -> Result<StatusCode, StatusCode> {
    state
        .domain
        .set_quota(&address, req.quota_bytes)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /v1/admin/accounts/{address}/recovery-email
pub async fn set_recovery_email(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
    Json(req): Json<wire::UpdateRecoveryEmailRequest>,
) -> Result<StatusCode, StatusCode> {
    state
        .domain
        .update_recovery_email(&address, &req.recovery_email)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /v1/admin/accounts/{address}/password — accepts a pre-hashed
/// password (webapi hashes locally, matching fastcore).
pub async fn set_account_password(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
    Json(req): Json<wire::SetPasswordRequest>,
) -> Result<StatusCode, StatusCode> {
    state
        .domain
        .set_password_hash(&address, &req.password_hash)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /v1/users/{user}/messages/{uid}/flags — set flags verbatim on
/// the message in the user's INBOX (parity with fastcore's per-user
/// uid index route).
pub async fn set_message_flags(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, uid)): Path<(String, u32)>,
    Json(req): Json<wire::SetMessageFlagsRequest>,
) -> Result<StatusCode, StatusCode> {
    use mailrs_mailbox::MailboxStore;
    let mb = state
        .mailbox
        .get_mailbox(&user, "INBOX")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    state
        .mailbox
        .set_flags(mb.id, uid, req.flags)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, uid, "set_message_flags failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── aliases ─────────────────────────────────────────────────────────

/// GET /v1/admin/aliases
pub async fn list_aliases(
    State(state): State<Arc<CoreRpcState>>,
) -> Result<Json<wire::AliasListResponse>, StatusCode> {
    let rows = state.domain.list_aliases().await.map_err(|e| {
        tracing::warn!(error = %e, "list_aliases failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let items = rows
        .into_iter()
        .map(|a| wire::AliasWire {
            id: a.id,
            source_address: a.source_address,
            target_address: a.target_address,
            domain: a.domain,
            alias_type: a.alias_type,
            active: a.active,
            created_at: a.created_at,
        })
        .collect();
    Ok(Json(wire::AliasListResponse { items }))
}

/// POST /v1/admin/aliases
pub async fn add_alias(
    State(state): State<Arc<CoreRpcState>>,
    Json(req): Json<wire::AddAliasRequest>,
) -> Result<Json<wire::AddAliasResponse>, StatusCode> {
    let now = chrono::Utc::now().timestamp();
    let id = state
        .domain
        .add_alias(
            &req.source_address,
            &req.target_address,
            &req.domain,
            &req.alias_type,
            now,
        )
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, source = %req.source_address, "add_alias failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(wire::AddAliasResponse { id }))
}

/// DELETE /v1/admin/aliases/{id}
pub async fn remove_alias(
    State(state): State<Arc<CoreRpcState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let removed = state.domain.remove_alias(id).await.map_err(|e| {
        tracing::warn!(error = %e, id, "remove_alias failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

// ── source-keyed alias API (v2 switchable-core boundary) ─────────────
// Both cores serve these identical routes: kevy is natively source-keyed,
// PG delegates to DomainStore::{upsert,remove}_alias_by_source. This is
// the backend-neutral alias surface webapi + mailrs-core-sync drive.

#[derive(serde::Deserialize)]
pub struct LocalAliasBody {
    pub source: String,
    pub target: String,
}

/// GET /v1/admin/aliases:local — `{ items: [{source, target}] }`.
pub async fn list_local_aliases(
    State(state): State<Arc<CoreRpcState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let rows = state.domain.list_aliases().await.map_err(|e| {
        tracing::warn!(error = %e, "list_local_aliases failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|a| serde_json::json!({"source": a.source_address, "target": a.target_address}))
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

/// POST /v1/admin/aliases:local — insert/replace one alias by source.
pub async fn upsert_local_alias(
    State(state): State<Arc<CoreRpcState>>,
    Json(body): Json<LocalAliasBody>,
) -> Result<StatusCode, StatusCode> {
    if body.source.is_empty() || body.target.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    state
        .domain
        .upsert_alias_by_source(&body.source, &body.target)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, source = %body.source, "upsert_local_alias failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /v1/admin/aliases:local/{source}
pub async fn delete_local_alias(
    State(state): State<Arc<CoreRpcState>>,
    Path(source): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .domain
        .remove_alias_by_source(&source)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, %source, "delete_local_alias failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── domains ─────────────────────────────────────────────────────────

/// GET /v1/admin/domains
pub async fn list_domains(
    State(state): State<Arc<CoreRpcState>>,
) -> Result<Json<wire::DomainListResponse>, StatusCode> {
    let rows = state.domain.list_domains().await.map_err(|e| {
        tracing::warn!(error = %e, "list_domains failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let items = rows
        .into_iter()
        .map(|d| wire::DomainWire {
            name: d.name,
            created_at: d.created_at,
        })
        .collect();
    Ok(Json(wire::DomainListResponse { items }))
}

/// POST /v1/admin/domains
pub async fn add_domain(
    State(state): State<Arc<CoreRpcState>>,
    Json(req): Json<wire::AddDomainRequest>,
) -> Result<StatusCode, StatusCode> {
    let now = chrono::Utc::now().timestamp();
    state.domain.add_domain(&req.name, now).await.map_err(|e| {
        tracing::warn!(error = %e, name = %req.name, "add_domain failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /v1/admin/domains/{name}
pub async fn remove_domain(
    State(state): State<Arc<CoreRpcState>>,
    Path(name): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let removed = state.domain.remove_domain(&name).await.map_err(|e| {
        tracing::warn!(error = %e, name = %name, "remove_domain failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

// ── sieve ───────────────────────────────────────────────────────────

/// POST /v1/admin/accounts/{address}/sieve
pub async fn set_sieve(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
    Json(req): Json<wire::SetSieveRequest>,
) -> Result<StatusCode, StatusCode> {
    let now = chrono::Utc::now().timestamp();
    state
        .domain
        .set_sieve_script(&address, &req.script, now)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, address = %address, "set_sieve failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /v1/admin/accounts/{address}/sieve
pub async fn delete_sieve(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .domain
        .delete_sieve_script(&address)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, address = %address, "delete_sieve failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── audit log ───────────────────────────────────────────────────────

// ── groups + permissions ────────────────────────────────────────────
