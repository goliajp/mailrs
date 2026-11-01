//! The alias-creation calls of SimpleLogin and addy.io, so a password
//! manager whose forwarder lets the user name the server (Bitwarden's
//! `simple-login.ts` and `addy-io.ts`) makes a masked address here. Only
//! that one call of each service; every mask is made by the same
//! `masked_email::create` as REST and JMAP, under the same rules, limits,
//! audit and tombstones.
//!
//! Only a key carrying the `masked_email` scope is accepted: not a
//! session, an app password, or a key that may do everything.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use mailrs_core_sidestate::families::masked_email::{Mask, NewMask, State as MaskState};
use serde_json::{Value, json};

use super::masked_email::{self as ops, MaskError};
use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::session::{AgentKey, AgentScopes};

/// SimpleLogin's path for a new random alias.
pub const SIMPLELOGIN_NEW: &str = "/api/alias/random/new";

type Reply = (StatusCode, Json<Value>);

/// The caller is a key with the `masked_email` scope.
fn mask_key(key: Option<&AgentKey>, scopes: &AgentScopes) -> bool {
    key.is_some() && scopes.0.iter().any(|s| s == "masked_email")
}

const NOT_A_MASK_KEY: &str = "this needs an API key that may manage masked addresses";

#[derive(Debug, serde::Deserialize)]
pub struct SimpleLoginQuery {
    #[serde(default)]
    pub hostname: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct SimpleLoginBody {
    #[serde(default)]
    pub note: String,
}

fn simplelogin_alias(m: &Mask) -> Value {
    json!({
        "alias": m.email,
        "email": m.email,
        "note": m.description,
        "enabled": m.state != MaskState::Disabled,
        "creation_timestamp": m.created_at,
    })
}

/// POST /api/alias/random/new[?hostname=] — SimpleLogin's random alias,
/// key in the `Authentication` header; 201 with `alias`.
pub async fn simplelogin_new(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Extension(scopes): Extension<AgentScopes>,
    key: Option<Extension<AgentKey>>,
    Query(q): Query<SimpleLoginQuery>,
    Json(body): Json<SimpleLoginBody>,
) -> Result<Reply, Reply> {
    let key = key.as_ref().map(|Extension(k)| k);
    if !mask_key(key, &scopes) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": NOT_A_MASK_KEY })),
        ));
    }
    let new = NewMask {
        for_domain: q.hostname.unwrap_or_default(),
        description: body.note,
        url: None,
        created_by: ops::made_by(key, "simplelogin"),
        state: None,
    };
    match ops::create(&state, &user, None, new).await {
        Ok(m) => Ok((StatusCode::CREATED, Json(simplelogin_alias(&m)))),
        Err(e) => Err((e.status(), Json(json!({ "error": e.describe() })))),
    }
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct AddyBody {
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub description: String,
}

fn addy_alias(m: &Mask) -> Value {
    let (local_part, domain) = m.email.rsplit_once('@').unwrap_or((m.email.as_str(), ""));
    json!({
        "data": {
            "id": m.id,
            "email": m.email,
            "local_part": local_part,
            "domain": domain,
            "description": m.description,
            "active": m.state != MaskState::Disabled,
            "created_at": chrono::DateTime::from_timestamp(m.created_at, 0)
                .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string()),
        }
    })
}

/// addy.io answers a field it will not take as a Laravel validation error.
fn addy_invalid(field: &str, message: &str) -> Reply {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({ "message": message, "errors": { field: [message] } })),
    )
}

/// POST /api/v1/aliases — addy.io's new alias on `domain`, which must be
/// one the account may use; 201 with `data.email`.
pub async fn addy_create(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Extension(scopes): Extension<AgentScopes>,
    key: Option<Extension<AgentKey>>,
    Json(body): Json<AddyBody>,
) -> Result<Reply, Reply> {
    let key = key.as_ref().map(|Extension(k)| k);
    if !mask_key(key, &scopes) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "message": NOT_A_MASK_KEY })),
        ));
    }
    if body.domain.trim().is_empty() {
        return Err(addy_invalid("domain", "The domain field is required."));
    }
    let new = NewMask {
        for_domain: String::new(),
        description: body.description,
        url: None,
        created_by: ops::made_by(key, "addy.io"),
        state: None,
    };
    match ops::create(&state, &user, Some(&body.domain), new).await {
        Ok(m) => Ok((StatusCode::CREATED, Json(addy_alias(&m)))),
        Err(MaskError::Domain) => Err(addy_invalid("domain", "The selected domain is invalid.")),
        Err(MaskError::Invalid(why)) => Err(addy_invalid("description", why)),
        Err(e) => Err((e.status(), Json(json!({ "message": e.describe() })))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask(state: MaskState) -> Mask {
        Mask {
            id: "id1".into(),
            email: "amber.otter.000042@x.jp".into(),
            owner: "me@x.jp".into(),
            state,
            for_domain: "shop.example".into(),
            description: "Generated by Bitwarden.".into(),
            url: None,
            created_at: 1_700_000_000,
            created_by: "key mk_abcd".into(),
            last_message_at: None,
        }
    }

    #[test]
    fn each_service_finds_the_address_where_its_client_reads_it() {
        let m = mask(MaskState::Enabled);
        assert_eq!(simplelogin_alias(&m)["alias"], "amber.otter.000042@x.jp");
        let a = addy_alias(&m);
        assert_eq!(a["data"]["email"], "amber.otter.000042@x.jp");
        assert_eq!(a["data"]["local_part"], "amber.otter.000042");
        assert_eq!(a["data"]["domain"], "x.jp");
        assert_eq!(a["data"]["created_at"], "2023-11-14 22:13:20");
        assert_eq!(
            simplelogin_alias(&mask(MaskState::Disabled))["enabled"],
            false
        );
    }

    #[test]
    fn only_a_key_with_the_mask_scope_is_taken() {
        let k = AgentKey {
            id: 1,
            prefix: "mk_abcd".into(),
            source: "127.0.0.1".parse().unwrap(),
        };
        let masked = AgentScopes(vec!["masked_email".into()]);
        assert!(mask_key(Some(&k), &masked));
        assert!(!mask_key(None, &masked), "a session");
        assert!(
            !mask_key(Some(&k), &AgentScopes::default()),
            "an unscoped key"
        );
        assert!(!mask_key(Some(&k), &AgentScopes(vec!["admin".into()])));
    }
}
