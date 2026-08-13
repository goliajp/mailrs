//! Accounts, aliases and domains — the directory the admin UI edits.

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

/// GET /api/admin/accounts
pub async fn list_accounts(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<wire::AccountListResponse>, StatusCode> {
    state.core.list_accounts().await.map(Json).map_err(map_err)
}

/// POST /api/admin/accounts — provision a new account. Writes an
/// AccountWithHashWire blob into fastcore-side kevy (via network kevy,
/// same key shape `mailrs:account:<addr>`) plus an empty
/// EffectivePermissionsResponse. Password is argon2-hashed here.
pub async fn add_account(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Json(req): Json<wire::AddAccountRequest>,
) -> Result<StatusCode, StatusCode> {
    // Delegate to fastcore — it owns the embedded kevy where accounts
    // live. Writing to the network kevy here (as we used to) landed on
    // a different store that fastcore never reads, so new accounts
    // could never log in. See audit Group B P0.
    state.core.add_account(&req).await.map_err(map_err)?;
    super::audit::record(&actor, "account.create", &req.address, "");
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/admin/accounts/{address} — remove account entries from
/// fastcore's embedded kevy. Does not touch maildir on disk — the
/// operator is responsible for cleaning that up if they want a hard
/// delete.
pub async fn remove_account(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    axum::extract::Path(address): axum::extract::Path<String>,
) -> Result<StatusCode, StatusCode> {
    state.core.remove_account(&address).await.map_err(map_err)?;
    super::audit::record(&actor, "account.delete", &address, "");
    Ok(StatusCode::NO_CONTENT)
}

/// One-shot boot-time mirror of network-kevy alias entries into the
/// fastcore-embedded alias table. Reads every `admin:aliases` hash row,
/// deserializes the `AliasWire`, and calls `fastcore.upsert_local_alias`
/// for each `source → target` pair. Idempotent — safe to run every boot.
/// Returns the count of aliases successfully mirrored.
pub async fn sync_aliases_to_fastcore(state: &Arc<WebState>) -> usize {
    let vals = match with_kevy(|c| hgetall_values(c, ALIAS_KEY)) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let mut synced = 0usize;
    for v in vals {
        let Ok(alias) = serde_json::from_slice::<wire::AliasWire>(&v) else {
            continue;
        };
        if !alias.active {
            continue;
        }
        if state
            .core
            .upsert_local_alias(&alias.source_address, &alias.target_address)
            .await
            .is_ok()
        {
            synced += 1;
        }
    }
    synced
}

/// GET /api/admin/aliases
///
/// v2.2-fix (2026-07-09): reads from the canonical fastcore-hosted
/// alias-store (`mailrs:aliases:index` set + per-source
/// `mailrs:alias:<addr>` string). The legacy `admin:aliases` hash
/// this used to walk was emptied when the alias data flipped to
/// network-kevy back-end (`project-alias-recovery-2026-07-05`), so
/// this handler returned `[]` regardless of the 40+ live aliases —
/// the admin panel showed "no aliases configured" even for a
/// super-admin. `id` is a deterministic i64 hash of `source` so the
/// frontend's delete-by-id semantic still works round-trip through
/// `remove_alias` below.
pub async fn list_aliases(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<wire::AliasListResponse>, StatusCode> {
    let raw = state
        .core
        .list_local_aliases()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let items: Vec<wire::AliasWire> = raw
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|it| {
            let source = it.get("source").and_then(|v| v.as_str())?.to_string();
            let target = it.get("target").and_then(|v| v.as_str())?.to_string();
            let domain = source.split('@').nth(1).unwrap_or("").to_string();
            Some(wire::AliasWire {
                id: stable_alias_id(&source),
                source_address: source,
                target_address: target,
                domain,
                alias_type: "alias".to_string(),
                active: true,
                created_at: 0,
            })
        })
        .collect();
    Ok(Json(wire::AliasListResponse { items }))
}

/// Deterministic i64 hash of an alias source address. Round-trippable
/// via `list → delete-by-id → find(id) → source` in `remove_alias`.
pub(crate) fn stable_alias_id(source: &str) -> i64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    source.hash(&mut h);
    // Cap at positive-i64 range so JSON round-trip through the frontend's
    // `number` type is lossless (JS safe-integer is 2^53 - 1, hash is
    // truncated below that ceiling).
    (h.finish() & 0x001F_FFFF_FFFF_FFFF) as i64
}

/// POST /api/admin/aliases — writes the `source → target` pair to
/// fastcore's shared alias-store (network kevy `mailrs:alias:<addr>`).
///
/// v2.2-fix (2026-07-09): the pre-fix version dual-wrote to a
/// `admin:aliases` hash + the canonical alias-store, but the list
/// endpoint only read from the hash — after the alias flip that hash
/// stayed empty in prod, so aliases added through the admin panel
/// appeared to vanish. Now single-writes to the canonical store;
/// `id` in the response is `stable_alias_id(source)` so the caller's
/// invalidate-and-refetch sees the same synthesized identity on the
/// next list.
pub async fn add_alias(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Json(req): Json<wire::AddAliasRequest>,
) -> Result<Json<wire::AddAliasResponse>, StatusCode> {
    state
        .core
        .upsert_local_alias(&req.source_address, &req.target_address)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // v2.2 (2026-07-09): domain index self-heal. Same reasoning as
    // fastcore's add_account_route — if the source address introduces
    // a domain we haven't indexed yet, the admin UI dropdowns would
    // miss it until the operator manually adds it. Idempotent; ignore
    // the error since the alias write already succeeded.
    if let Some((_, domain)) = req.source_address.split_once('@')
        && let Err(e) = state.core.add_domain(domain).await
    {
        tracing::warn!(err = %e, %domain, "add_domain self-heal from add_alias failed");
    }
    super::audit::record(
        &actor,
        "alias.create",
        &req.source_address,
        &format!("-> {}", req.target_address),
    );
    Ok(Json(wire::AddAliasResponse {
        id: stable_alias_id(&req.source_address),
    }))
}

/// DELETE /api/admin/aliases/{id} — resolves `id` back to the source
/// address via `stable_alias_id(source)` reverse-lookup over the
/// current alias list, then drops it from the canonical alias-store.
///
/// v2.2-fix (2026-07-09): pre-fix version resolved the source via the
/// legacy `admin:aliases` hash (empty in prod), so every delete
/// call 404'd silently and left the alias in place. Now walks
/// fastcore's live list to find the matching source and calls
/// `delete_local_alias(source)`.
pub async fn remove_alias(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let raw = state
        .core
        .list_local_aliases()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let source = raw
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find_map(|it| {
            let s = it.get("source").and_then(|v| v.as_str())?.to_string();
            (stable_alias_id(&s) == id).then_some(s)
        })
        .ok_or(StatusCode::NOT_FOUND)?;
    state
        .core
        .delete_local_alias(&source)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    super::audit::record(&actor, "alias.delete", &source, "");
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/admin/domains
///
/// v2.2-fix (2026-07-09): read through fastcore RPC (embedded kevy
/// `mailrs:domains:index` set + per-name entries), same reason the
/// alias handler moved above — the legacy `admin:domains` network-kevy
/// hash was emptied at the fastcore split and returned `[]`, breaking
/// the alias-create form's domain dropdown and every downstream
/// domain-gated UI.
pub async fn list_domains(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<wire::DomainListResponse>, StatusCode> {
    state.core.list_domains().await.map(Json).map_err(map_err)
}

#[derive(Debug, serde::Deserialize)]
pub struct AddDomainBody {
    pub name: String,
}

/// POST /api/admin/domains — writes through fastcore RPC.
pub async fn add_domain(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Json(req): Json<AddDomainBody>,
) -> Result<StatusCode, StatusCode> {
    state.core.add_domain(&req.name).await.map_err(map_err)?;
    super::audit::record(&actor, "domain.create", &req.name, "");
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/admin/domains/{name} — deletes through fastcore RPC.
pub async fn remove_domain(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<StatusCode, StatusCode> {
    state.core.remove_domain(&name).await.map_err(map_err)?;
    super::audit::record(&actor, "domain.delete", &name, "");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
pub struct UpdateAccountRequest {
    pub display_name: Option<String>,
    pub recovery_email: Option<String>,
    pub disabled: Option<bool>,
}

/// PUT /api/admin/accounts/{address} — patch account fields via
/// fastcore. `display_name` goes through the dedicated RPC;
/// `recovery_email` reuses `set_recovery_email`. `disabled` is
/// currently a no-op fastcore-side (tracked in a follow-up — needs a
/// new field on the account blob).
pub async fn update_account(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(req): Json<UpdateAccountRequest>,
) -> Result<StatusCode, StatusCode> {
    let mut fields_changed = Vec::new();
    if let Some(dn) = req.display_name {
        let wire_req = wire::UpdateAccountRequest {
            display_name: dn.clone(),
        };
        state
            .core
            .update_account(&address, &wire_req)
            .await
            .map_err(map_err)?;
        fields_changed.push(format!("display_name={dn}"));
    }
    if let Some(re) = req.recovery_email {
        let wire_req = wire::UpdateRecoveryEmailRequest {
            recovery_email: re.clone(),
        };
        state
            .core
            .set_recovery_email(&address, &wire_req)
            .await
            .map_err(map_err)?;
        fields_changed.push(format!("recovery_email={re}"));
    }
    // `disabled` — TODO: needs a dedicated field/route on fastcore.
    // Silently ignored for now.
    if !fields_changed.is_empty() {
        super::audit::record(
            &actor,
            "account.update",
            &address,
            &fields_changed.join(","),
        );
    }
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/admin/accounts/{address}/quota — return the stored quota
/// (bytes) if present, else `null`. Quota lives inside the account
/// blob under `quota_bytes` (i64).
pub async fn get_account_quota(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("mailrs:account:{address}");
    let cur = with_kevy(move |c| {
        c.hget(key.as_bytes(), b"blob")
            .map_err(std::io::Error::from)
    })?;
    let Some(cur) = cur else {
        return Ok(Json(serde_json::json!({ "quota_bytes": null })));
    };
    let val: serde_json::Value =
        serde_json::from_slice(&cur).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let quota = val
        .get("quota_bytes")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    Ok(Json(serde_json::json!({ "quota_bytes": quota })))
}

#[derive(Debug, serde::Deserialize)]
pub struct SetQuotaRequest {
    pub quota_bytes: i64,
}

/// POST /api/admin/accounts/{address}/quota — patch `quota_bytes` via
/// fastcore RPC. `-1` sentinel means unlimited.
pub async fn set_account_quota(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(req): Json<SetQuotaRequest>,
) -> Result<StatusCode, StatusCode> {
    let wire_req = wire::SetQuotaRequest {
        quota_bytes: req.quota_bytes,
    };
    state
        .core
        .set_quota(&address, &wire_req)
        .await
        .map_err(map_err)?;
    super::audit::record(
        &actor,
        "account.quota",
        &address,
        &format!("{} bytes", req.quota_bytes),
    );
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/admin/accounts/{address}/groups — group memberships from
/// admin:groups:<gid>:members set membership check.
pub async fn list_account_groups(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let addr_c = address.clone();
    // Read the account's own membership set: admin:account:<addr>:groups
    let key = format!("admin:account:{addr_c}:groups");
    let members = with_kevy(move |c| c.smembers(key.as_bytes()).map_err(std::io::Error::from))?;
    let groups: Vec<String> = members
        .into_iter()
        .filter_map(|v| String::from_utf8(v).ok())
        .collect();
    Ok(Json(serde_json::json!({ "groups": groups })))
}

pub async fn get_account_overrides(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("admin:account:{address}:overrides");
    let val = with_kevy(move |c| c.get(key.as_bytes()).map_err(std::io::Error::from))?;
    let parsed: serde_json::Value = val
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    Ok(Json(parsed))
}

pub async fn set_account_overrides(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("admin:account:{address}:overrides");
    let payload = serde_json::to_vec(&req).map_err(|_| StatusCode::BAD_REQUEST)?;
    let payload_c = payload.clone();
    with_kevy(move |c| {
        c.set(key.as_bytes(), &payload_c)?;
        Ok(())
    })?;
    super::audit::record(
        &actor,
        "overrides.update",
        &address,
        &String::from_utf8(payload).unwrap_or_default(),
    );
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_email_group_members(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("admin:email-group:{id}:members");
    let members = with_kevy(move |c| c.smembers(key.as_bytes()).map_err(std::io::Error::from))?;
    let items: Vec<String> = members
        .into_iter()
        .filter_map(|v| String::from_utf8(v).ok())
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, serde::Deserialize)]
pub struct AddMemberRequest {
    pub address: String,
}

pub async fn add_email_group_member(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(id): Path<String>,
    Json(req): Json<AddMemberRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("admin:email-group:{id}:members");
    let addr = req.address;
    let addr_c = addr.clone();
    with_kevy(move |c| {
        c.sadd(key.as_bytes(), &[addr_c.as_bytes()])?;
        Ok(())
    })?;
    super::audit::record(&actor, "email_group.member_add", &id, &addr);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_email_group_member(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path((id, address)): Path<(String, String)>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("admin:email-group:{id}:members");
    let address_c = address.clone();
    with_kevy(move |c| {
        c.srem(key.as_bytes(), &[address_c.as_bytes()])?;
        Ok(())
    })?;
    super::audit::record(&actor, "email_group.member_remove", &id, &address);
    Ok(StatusCode::NO_CONTENT)
}
