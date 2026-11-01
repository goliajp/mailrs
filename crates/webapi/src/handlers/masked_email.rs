//! Masked addresses over REST, and the operations JMAP `MaskedEmail/*`
//! shares with it: one random address per site, delivering to the account.
//!
//! The store is `core_sidestate::families::masked_email`; what it cannot
//! see — whether an address is an account, an alias or a group — is asked
//! here before a candidate is written. Every create, state change and delete
//! is audited with the address's first eight characters and the site.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use mailrs_core_sidestate::families::masked_email::{
    self as store, CreateError, Mask, NewMask, Patch, State as MaskState, UpdateError,
};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;
use crate::session::AgentKey;

/// Why a mask operation was refused, as the REST status and JMAP error.
#[derive(Debug)]
pub(crate) enum MaskError {
    /// The domain asked for is not one the account may use.
    Domain,
    TooMany,
    TooManyToday,
    NotFound,
    /// A deleted mask stays deleted.
    Deleted,
    /// A value the caller sent cannot be stored.
    Invalid(&'static str),
    Unavailable,
}

impl MaskError {
    pub(crate) fn status(&self) -> StatusCode {
        match self {
            MaskError::Domain => StatusCode::FORBIDDEN,
            MaskError::TooMany | MaskError::Deleted => StatusCode::CONFLICT,
            MaskError::TooManyToday => StatusCode::TOO_MANY_REQUESTS,
            MaskError::NotFound => StatusCode::NOT_FOUND,
            MaskError::Invalid(_) => StatusCode::BAD_REQUEST,
            MaskError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    pub(crate) fn describe(&self) -> &'static str {
        match self {
            MaskError::Domain => "the account may not use that domain",
            MaskError::TooMany => "the account has as many masked addresses as it may",
            MaskError::TooManyToday => "the account made as many masked addresses today as it may",
            MaskError::NotFound => "no such masked address",
            MaskError::Deleted => "a deleted masked address cannot be changed",
            MaskError::Invalid(why) => why,
            MaskError::Unavailable => "the store could not be reached",
        }
    }
}

/// Longest site, description or URL kept.
const MAX_TEXT: usize = 512;

fn text(v: &str, what: &'static str) -> Result<String, MaskError> {
    match v.len() > MAX_TEXT || v.contains(['\r', '\n']) {
        true => Err(MaskError::Invalid(what)),
        false => Ok(v.trim().to_string()),
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The first eight characters, which is all the audit log keeps.
fn prefix(email: &str) -> String {
    email.chars().take(8).collect()
}

/// The domains `user` may make masks on: its own, and those of the
/// addresses it may send as.
pub(crate) async fn allowed_domains(state: &WebState, user: &str) -> Vec<String> {
    let mut out: Vec<String> = user
        .rsplit_once('@')
        .map(|(_, d)| d.to_ascii_lowercase())
        .into_iter()
        .collect();
    if let Ok(perms) = state.core.effective_permissions(user).await {
        for a in &perms.send_as {
            if let Some((_, d)) = mailrs_rfc5322::addr_spec(a).rsplit_once('@') {
                let d = d.to_ascii_lowercase();
                if !out.contains(&d) {
                    out.push(d);
                }
            }
        }
    }
    out
}

/// Whether `email` is an account, an explicit alias or a group.
async fn taken_elsewhere(state: &WebState, email: &str) -> Result<bool, MaskError> {
    match state.core.get_account_with_hash(email).await {
        Ok(_) => return Ok(true),
        Err(e) if e.status_code() == 404 => {}
        Err(_) => return Err(MaskError::Unavailable),
    }
    let aliases = state
        .core
        .list_local_aliases()
        .await
        .map_err(|_| MaskError::Unavailable)?;
    let aliased = aliases
        .get("items")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|it| it.get("source").and_then(|s| s.as_str()))
        .any(|s| s.eq_ignore_ascii_case(email));
    if aliased {
        return Ok(true);
    }
    let addr = email.to_string();
    with_kevy(move |c| mailrs_core_sidestate::families::email_groups::find_by_address(c, &addr))
        .map(|g| g.is_some())
        .map_err(|_| MaskError::Unavailable)
}

/// Make a mask for `user` on `domain` (its own when `None`).
pub(crate) async fn create(
    state: &WebState,
    user: &str,
    domain: Option<&str>,
    new: NewMask,
) -> Result<Mask, MaskError> {
    let domains = allowed_domains(state, user).await;
    let domain = match domain {
        Some(d) => d.trim().to_ascii_lowercase(),
        None => domains.first().cloned().ok_or(MaskError::Domain)?,
    };
    if !domains.contains(&domain) {
        return Err(MaskError::Domain);
    }
    let new = NewMask {
        for_domain: text(&new.for_domain, "forDomain is too long")?,
        description: text(&new.description, "description is too long")?,
        url: new.url.map(|u| text(&u, "url is too long")).transpose()?,
        ..new
    };
    for _ in 0..store::TRIES {
        let email = store::candidate(&domain, store::os_pick);
        if taken_elsewhere(state, &email).await? {
            continue;
        }
        let (owner, cand, n) = (user.to_string(), email.clone(), new.clone());
        let made = with_kevy(move |c| Ok(store::create_at(c, &owner, &cand, &n, now())))
            .map_err(|_| MaskError::Unavailable)?;
        match made {
            Ok(m) => {
                crate::handlers::audit::record(
                    user,
                    "mask.create",
                    &prefix(&m.email),
                    &m.for_domain,
                );
                return Ok(m);
            }
            Err(CreateError::Taken) => continue,
            Err(CreateError::TooMany) => return Err(MaskError::TooMany),
            Err(CreateError::TooManyToday) => return Err(MaskError::TooManyToday),
            Err(CreateError::NoFreeAddress | CreateError::Store(_)) => {
                return Err(MaskError::Unavailable);
            }
        }
    }
    Err(MaskError::Unavailable)
}

/// `user`'s live masks.
pub(crate) fn list(user: &str) -> Result<Vec<Mask>, MaskError> {
    let owner = user.to_string();
    with_kevy(move |c| store::list(c, &owner)).map_err(|_| MaskError::Unavailable)
}

/// Change `user`'s mask at `email`; setting `deleted` makes it a tombstone.
pub(crate) fn update(user: &str, email: &str, patch: Patch) -> Result<Mask, MaskError> {
    let patch = Patch {
        for_domain: patch
            .for_domain
            .map(|d| text(&d, "forDomain is too long"))
            .transpose()?,
        description: patch
            .description
            .map(|d| text(&d, "description is too long"))
            .transpose()?,
        url: patch
            .url
            .map(|u| u.map(|u| text(&u, "url is too long")).transpose())
            .transpose()?,
        ..patch
    };
    let changes_state = patch.state;
    let (owner, addr) = (user.to_string(), email.to_string());
    let done = with_kevy(move |c| Ok(store::update(c, &owner, &addr, patch)))
        .map_err(|_| MaskError::Unavailable)?;
    match done {
        Ok(m) => {
            if let Some(s) = changes_state {
                let action = match s {
                    MaskState::Deleted => "mask.delete",
                    MaskState::Disabled => "mask.disable",
                    MaskState::Enabled | MaskState::Pending => "mask.enable",
                };
                crate::handlers::audit::record(user, action, &prefix(&m.email), &m.for_domain);
            }
            Ok(m)
        }
        Err(UpdateError::NotFound) => Err(MaskError::NotFound),
        Err(UpdateError::Deleted) => Err(MaskError::Deleted),
        Err(UpdateError::Store(_)) => Err(MaskError::Unavailable),
    }
}

fn refused(e: MaskError) -> (StatusCode, Json<serde_json::Value>) {
    (
        e.status(),
        Json(serde_json::json!({ "error": e.describe() })),
    )
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBody {
    #[serde(default)]
    pub for_domain: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub url: Option<String>,
    /// One of the account's domains; its own when absent.
    #[serde(default)]
    pub domain: Option<String>,
}

/// What made a mask: a key's prefix, or the web.
fn made_by(key: Option<&AgentKey>, web: &str) -> String {
    match key {
        Some(k) => format!("key {}", k.prefix),
        None => web.to_string(),
    }
}

/// GET /api/masked-emails — the account's live masks, newest first.
pub async fn list_masks(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vec<Mask>>, (StatusCode, Json<serde_json::Value>)> {
    list(&user).map(Json).map_err(refused)
}

/// POST /api/masked-emails — a new mask; 201 with it.
pub async fn create_mask(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    key: Option<Extension<AgentKey>>,
    Json(body): Json<CreateBody>,
) -> Result<(StatusCode, Json<Mask>), (StatusCode, Json<serde_json::Value>)> {
    let new = NewMask {
        for_domain: body.for_domain,
        description: body.description,
        url: body.url,
        created_by: made_by(key.as_ref().map(|Extension(k)| k), "web"),
        state: None,
    };
    create(&state, &user, body.domain.as_deref(), new)
        .await
        .map(|m| (StatusCode::CREATED, Json(m)))
        .map_err(refused)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBody {
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub for_domain: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// `null` clears it; absent leaves it.
    #[serde(default, with = "serde_with_null")]
    pub url: Option<Option<String>>,
}

/// `url: null` is a change, an absent `url` is not.
mod serde_with_null {
    pub fn deserialize<'de, D>(d: D) -> Result<Option<Option<String>>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        serde::Deserialize::deserialize(d).map(Some)
    }
}

/// The state a caller may set; `pending` is only for creation.
pub(crate) fn settable_state(s: &str) -> Result<MaskState, MaskError> {
    match MaskState::parse(s) {
        Some(MaskState::Pending) | None => Err(MaskError::Invalid("unknown state")),
        Some(st) => Ok(st),
    }
}

/// PATCH /api/masked-emails/{email} — change its state, site or note.
pub async fn update_mask(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(email): Path<String>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<Mask>, (StatusCode, Json<serde_json::Value>)> {
    let state = body
        .state
        .as_deref()
        .map(settable_state)
        .transpose()
        .map_err(refused)?;
    let patch = Patch {
        state,
        for_domain: body.for_domain,
        description: body.description,
        url: body.url,
    };
    update(&user, &email, patch).map(Json).map_err(refused)
}

/// DELETE /api/masked-emails/{email} — deleted for good: it takes no more
/// mail and is never issued again.
pub async fn delete_mask(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(email): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let patch = Patch {
        state: Some(MaskState::Deleted),
        ..Patch::default()
    };
    update(&user, &email, patch)
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(refused)
}

/// 409 when `address` is or was a masked address: an account, alias or
/// group there would take a site's mail meant for the mask's owner.
pub(crate) fn refuse_if_masked(address: &str) -> Result<(), StatusCode> {
    let addr = mailrs_rfc5322::addr_spec(address).trim().to_string();
    match with_kevy(move |c| store::is_taken(c, &addr))? {
        true => Err(StatusCode::CONFLICT),
        false => Ok(()),
    }
}
