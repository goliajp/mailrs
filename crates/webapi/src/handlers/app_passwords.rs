//! App passwords: per-device passwords for mail apps, made and revoked by
//! the account's owner; and, for a super administrator, who still signs in
//! to the mail protocols with the account password, and each account's
//! setting for whether that password still works there.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use mailrs_auth_guard::app_passwords::{self, Policy, Refused};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// GET /api/auth/app-passwords — the caller's, newest first; never the
/// passwords themselves.
pub async fn list(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vec<app_passwords::Info>>, StatusCode> {
    Ok(Json(with_kevy(move |c| app_passwords::list(c, &user))?))
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateRequest {
    pub name: String,
}

/// POST /api/auth/app-passwords — make one. The password is in this answer
/// only. Needs a recent password confirmation; the owner is told.
pub async fn create(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    crate::login_guard::ClientIp(source): crate::login_guard::ClientIp,
    headers: HeaderMap,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<CreateRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let (u, name) = (user.clone(), req.name.clone());
    let made = with_kevy(move |c| app_passwords::create(c, &u, &name, now()))
        .map_err(|s| (s, Json(serde_json::json!({ "error": "store_unavailable" }))))?;
    let (id, password) = match made {
        Ok(v) => v,
        Err(Refused::TooMany) => {
            return Err((
                StatusCode::CONFLICT,
                Json(
                    serde_json::json!({ "error": "too_many", "max": app_passwords::MAX_PER_ACCOUNT }),
                ),
            ));
        }
        Err(Refused::BadName) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "bad_name" })),
            ));
        }
    };
    let name = req.name.trim().to_string();
    crate::handlers::audit::record(
        &user,
        "app_password.create",
        &user,
        &format!("id={id} name={name}"),
    );
    crate::handlers::owner_notice::notify_owner(
        Some(&state),
        &user,
        crate::security_mail::Notice::AppPasswordMade(name.clone()),
        crate::handlers::owner_notice::origin_of(&headers, &source.to_string()),
        &[],
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "id": id, "name": name, "password": password })),
    ))
}

/// DELETE /api/auth/app-passwords/{id} — revoke one of the caller's.
pub async fn revoke(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let (u, i) = (user.clone(), id.clone());
    if !with_kevy(move |c| app_passwords::revoke(c, &u, &i))? {
        return Err(StatusCode::NOT_FOUND);
    }
    crate::handlers::audit::record(&user, "app_password.revoke", &user, &format!("id={id}"));
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/auth/app-passwords/revoke-all — revoke all of the caller's.
pub async fn revoke_all(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let u = user.clone();
    let n = with_kevy(move |c| app_passwords::revoke_all(c, &u))?;
    crate::handlers::audit::record(
        &user,
        "app_password.revoke_all",
        &user,
        &format!("count={n}"),
    );
    Ok(Json(serde_json::json!({ "revoked": n })))
}

#[derive(Debug, serde::Deserialize)]
pub struct UsageQuery {
    #[serde(default = "thirty")]
    pub days: u64,
}

fn thirty() -> u64 {
    30
}

/// GET /api/admin/protocol-logins?days=30 — successful mail-protocol
/// sign-ins per account and protocol, with the account password and with
/// app passwords, and each account's setting.
pub async fn usage(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Query(q): Query<UsageQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    crate::handlers::kevy_util::require_super(&state, &actor).await?;
    let rows = crate::login_guard::GUARD
        .protocol_usage(q.days)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "protocol usage: store unreachable");
            StatusCode::SERVICE_UNAVAILABLE
        })?;
    let accounts: Vec<String> = {
        let mut a: Vec<String> = rows.iter().map(|r| r.account.clone()).collect();
        a.dedup();
        a
    };
    let policies = with_kevy(move |c| {
        accounts
            .into_iter()
            .map(|a| app_passwords::policy(c, &a).map(|p| (a, p.as_str())))
            .collect::<std::io::Result<std::collections::BTreeMap<_, _>>>()
    })?;
    let default = app_passwords::default_policy().map_err(|e| {
        tracing::error!(error = %e, "protocol usage: bad default");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(serde_json::json!({
        "days": q.days.min(mailrs_auth_guard::events::RETAIN_DAYS),
        "default": default.as_str(),
        "policies": policies,
        "items": rows,
    })))
}

#[derive(Debug, serde::Deserialize)]
pub struct PolicyRequest {
    pub policy: String,
}

/// PUT /api/admin/accounts/{address}/protocol-password — whether the
/// account's own password still signs in to the mail protocols:
/// `inherit`, `allow` or `app-only`.
pub async fn set_policy(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(req): Json<PolicyRequest>,
) -> Result<StatusCode, StatusCode> {
    crate::handlers::kevy_util::require_super(&state, &actor).await?;
    let policy = Policy::parse(&req.policy).ok_or(StatusCode::BAD_REQUEST)?;
    let address = address.trim().to_ascii_lowercase();
    let a = address.clone();
    with_kevy(move |c| app_passwords::set_policy(c, &a, policy))?;
    crate::handlers::audit::record(&actor, "app_password.policy", &address, policy.as_str());
    Ok(StatusCode::NO_CONTENT)
}
