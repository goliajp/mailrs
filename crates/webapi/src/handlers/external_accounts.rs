//! `/api/accounts/external/*` — the mailboxes somewhere else.
//!
//! Storage on network kevy, via
//! `mailrs_core_sidestate::families::external_accounts`:
//!
//!   ext:accts:{user}        hash    id → JSON AccountRow
//!   ext:secret:{user}:{id}  string  sealed by mailrs-secretbox
//!
//! **The secret goes in and never comes out.** It is accepted on create
//! and on re-authenticate, sealed immediately, and no route returns it
//! — not even to the owner, who has it already and would only be
//! putting it through another log.

use axum::Json;
use axum::extract::{Extension, Path};
use axum::http::StatusCode;
use mailrs_core_sidestate::families::external_accounts as ext;
use mailrs_mailprovider::{Autodiscover, preset_for};
use serde::Deserialize;

use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

mod keys {
    pub fn accounts(user: &str) -> String {
        format!("ext:accts:{user}")
    }
    pub fn secret(user: &str, id: &str) -> String {
        format!("ext:secret:{user}:{id}")
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The deployment's sealing key, or the reason there is none.
///
/// Absent means external accounts do not work, and every route says so
/// in the same words. It does **not** mean secrets are stored in the
/// clear — that would be a silent downgrade of the one thing this
/// module exists to protect.
fn sealing_key() -> Result<mailrs_secretbox::Key, (StatusCode, String)> {
    match std::env::var("MAILRS_ACCOUNT_KEY") {
        Ok(v) if !v.trim().is_empty() => Ok(mailrs_secretbox::Key::from_passphrase(&v)),
        _ => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "MAILRS_ACCOUNT_KEY is not set on this server, so an external \
             account's password cannot be stored safely and none can be added"
                .into(),
        )),
    }
}

/// What a client may set. The row's own bookkeeping is not in here.
#[derive(Debug, Deserialize)]
pub struct NewAccount {
    pub email: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub incoming: Option<ext::Endpoint>,
    #[serde(default)]
    pub outgoing: Option<ext::Endpoint>,
    #[serde(default)]
    pub auth: Option<ext::AuthKind>,
    #[serde(default)]
    pub username: Option<String>,
    /// The password, app password or refresh token. Sealed on arrival.
    #[serde(default)]
    pub secret: Option<String>,
}

/// `GET /api/accounts/external` — the user's accounts, in display order.
pub async fn list(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let rows = load(&user)?;
    Ok(Json(serde_json::json!({ "accounts": rows })))
}

/// `POST /api/accounts/external` — add one.
///
/// The preset fills in whatever the client did not send, so a phone can
/// post an address and a password and get a working account.
pub async fn create(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(body): Json<NewAccount>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let key = sealing_key()?;
    let mut row = build_row(&body, now_secs());
    // A domain with no preset gets looked up rather than guessed at.
    // Without this the row was written with an empty host and failed
    // every sync afterwards, saying nothing a person could act on.
    if row.incoming.host.trim().is_empty() || row.outgoing.host.trim().is_empty() {
        discover_into(&mut row).await;
    }
    ext::validate(&row).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let secret = body.secret.as_deref().filter(|s| !s.is_empty()).ok_or((
        StatusCode::BAD_REQUEST,
        "no password or authorisation code was given".to_string(),
    ))?;
    let sealed = mailrs_secretbox::seal(&key, secret.as_bytes())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let (accounts_key, secret_key) = (keys::accounts(&user), keys::secret(&user, &row.id));
    let json = serde_json::to_string(&row).unwrap_or_default();
    let id = row.id.clone();
    with_kevy(move |c| {
        c.set(secret_key.as_bytes(), sealed.as_bytes())?;
        c.hset(accounts_key.as_bytes(), &[(id.as_bytes(), json.as_bytes())])?;
        Ok(())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::to_value(&row).unwrap_or_default()))
}

/// Store an account connected by OAuth, with its tokens sealed.
///
/// **One sealed blob, not two secrets.** An access token and a refresh
/// token are two things that can be half-deleted, half-rotated or
/// half-read; keeping them in one JSON value under one key means they
/// arrive and leave together, which is the only state either is useful
/// in.
pub(crate) fn connect_oauth_account(
    user: &str,
    provider_key: &str,
    email: &str,
    access_token: &str,
    refresh_token: &str,
    expires_in: i64,
) -> Result<(), (StatusCode, String)> {
    let key = sealing_key()?;
    let now = now_secs();
    let preset = mailrs_mailprovider::preset_for_domain(
        email.rsplit_once('@').map(|(_, d)| d).unwrap_or_default(),
    );
    let id = format!("ext_{now}_{}", stable_suffix(email));
    let row = ext::AccountRow {
        colour: Some(ext::colour_for(&id).to_string()),
        id: id.clone(),
        email: email.to_string(),
        display_name: email.to_string(),
        provider: provider_key.to_string(),
        incoming: preset.map(|p| endpoint_of(&p.imap)).unwrap_or_default(),
        outgoing: preset.map(|p| endpoint_of(&p.smtp)).unwrap_or_default(),
        auth: ext::AuthKind::OAuth2,
        created_at: now,
        sort: now,
        ..ext::AccountRow::default()
    };
    ext::validate(&row).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    // The absolute instant, not the duration: a stored `expires_in`
    // means nothing an hour after it was written, and the worker asks
    // "is it due" rather than "how long was it good for".
    let sealed = mailrs_secretbox::seal(
        &key,
        serde_json::json!({
            "access_token": access_token,
            "refresh_token": refresh_token,
            "expires_at": now + expires_in,
        })
        .to_string()
        .as_bytes(),
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let (accounts_key, secret_key) = (keys::accounts(user), keys::secret(user, &id));
    let json = serde_json::to_string(&row).unwrap_or_default();
    with_kevy(move |c| {
        c.set(secret_key.as_bytes(), sealed.as_bytes())?;
        c.hset(accounts_key.as_bytes(), &[(id.as_bytes(), json.as_bytes())])?;
        Ok(())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(())
}

/// `DELETE /api/accounts/external/{id}` — remove one, and its secret.
pub async fn delete(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let (accounts_key, secret_key) = (keys::accounts(&user), keys::secret(&user, &id));
    with_kevy(move |c| {
        c.hdel(accounts_key.as_bytes(), &[id.as_bytes()])?;
        // The secret goes with it. A row removed while its sealed
        // token stays behind is a credential nobody can see and nobody
        // will delete.
        c.del(&[secret_key.as_bytes()])?;
        Ok(())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/accounts/external/settings?email=…` — what a set-up screen
/// should fill in, before anything is saved.
///
/// Answers with a preset when there is one and with the autodiscovery
/// steps when there is not, so the screen can show its guess and let
/// every field be corrected rather than failing at the first connect.
pub async fn settings_for(
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let email = q.get("email").map(String::as_str).unwrap_or_default();
    let domain = email.rsplit_once('@').map(|(_, d)| d).unwrap_or(email);
    if let Some(p) = preset_for(email).or_else(|| mailrs_mailprovider::preset_for_domain(domain)) {
        return Ok(Json(serde_json::json!({
            "known": true,
            "preset": {
                "id": p.id, "label": p.label, "auth": p.auth,
                "imap": p.imap, "smtp": p.smtp,
                "secret_help": p.secret_help,
                "skip_folders": p.skip_folders,
            }
        })));
    }
    let steps: Vec<serde_json::Value> = Autodiscover::for_domain(domain)
        .into_iter()
        .map(|s| match s {
            Autodiscover::Srv {
                name,
                protocol,
                tls,
            } => serde_json::json!({
                "kind": "srv", "name": name, "protocol": protocol, "tls": tls
            }),
            Autodiscover::Ispdb { url } => serde_json::json!({ "kind": "ispdb", "url": url }),
            Autodiscover::Guess { imap, smtp } => serde_json::json!({
                "kind": "guess", "imap": imap, "smtp": smtp
            }),
        })
        .collect();
    Ok(Json(
        serde_json::json!({ "known": false, "autodiscover": steps }),
    ))
}

fn load(user: &str) -> Result<Vec<ext::AccountRow>, (StatusCode, String)> {
    let k = keys::accounts(user);
    let flat = with_kevy(move |c| c.hgetall(k.as_bytes()).map_err(std::io::Error::from))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    // `hgetall` answers a flat field/value list, not pairs.
    let mut rows: Vec<ext::AccountRow> = Vec::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        // A row that will not parse is skipped rather than failing the
        // list: one bad row must not hide the other four accounts.
        if let Ok(row) = serde_json::from_slice::<ext::AccountRow>(&flat[i + 1]) {
            rows.push(row);
        }
        i += 2;
    }
    rows.sort_by(|a, b| {
        a.sort
            .cmp(&b.sort)
            .then_with(|| a.created_at.cmp(&b.created_at))
    });
    Ok(rows)
}

fn build_row(body: &NewAccount, now: i64) -> ext::AccountRow {
    let preset = preset_for(&body.email);
    let id = format!("ext_{now}_{}", stable_suffix(&body.email));
    ext::AccountRow {
        colour: Some(ext::colour_for(&id).to_string()),
        id,
        display_name: if body.display_name.is_empty() {
            body.email.clone()
        } else {
            body.display_name.clone()
        },
        provider: body
            .provider
            .clone()
            .or_else(|| preset.map(|p| p.id.to_string()))
            .unwrap_or_else(|| "custom".into()),
        incoming: body
            .incoming
            .clone()
            .or_else(|| preset.map(|p| endpoint_of(&p.imap)))
            .unwrap_or_default(),
        outgoing: body
            .outgoing
            .clone()
            .or_else(|| preset.map(|p| endpoint_of(&p.smtp)))
            .unwrap_or_default(),
        auth: body
            .auth
            .or_else(|| preset.map(|p| auth_of(p.auth)))
            .unwrap_or_default(),
        username: body.username.clone(),
        email: body.email.clone(),
        created_at: now,
        sort: now,
        ..ext::AccountRow::default()
    }
}

fn endpoint_of(e: &mailrs_mailprovider::Endpoint) -> ext::Endpoint {
    ext::Endpoint {
        protocol: match e.protocol {
            mailrs_mailprovider::Protocol::Imap => "imap",
            mailrs_mailprovider::Protocol::Pop3 => "pop3",
            mailrs_mailprovider::Protocol::Jmap => "jmap",
            mailrs_mailprovider::Protocol::Smtp => "smtp",
        }
        .into(),
        host: e.host.to_string(),
        port: e.port,
        tls: match e.tls {
            mailrs_mailprovider::Tls::Implicit => ext::Tls::Implicit,
            mailrs_mailprovider::Tls::StartTls => ext::Tls::StartTls,
            mailrs_mailprovider::Tls::None => ext::Tls::None,
        },
    }
}

fn auth_of(a: mailrs_mailprovider::AuthKind) -> ext::AuthKind {
    match a {
        mailrs_mailprovider::AuthKind::Password => ext::AuthKind::Password,
        mailrs_mailprovider::AuthKind::AppPassword => ext::AuthKind::AppPassword,
        mailrs_mailprovider::AuthKind::OAuth2 => ext::AuthKind::OAuth2,
    }
}

/// Eight hex characters from the address, so two accounts added in the
/// same second do not collide.
fn stable_suffix(v: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in v.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    format!("{:08x}", h as u32)
}

/// Fill a row's servers from DNS, then from the conventional names.
///
/// The order is the point: a provider's own SRV records are
/// authoritative and the conventional hostname is a guess. Trying the
/// guess first ships settings that appear to work until they do not.
///
/// Whatever it ends with, the person can correct on the form — a guess
/// offered and editable beats a failure with no next step.
async fn discover_into(row: &mut ext::AccountRow) {
    let Some(domain) = row.email.rsplit_once('@').map(|(_, d)| d.to_string()) else {
        return;
    };
    let resolver = hickory_resolver::TokioResolver::builder_tokio()
        .ok()
        .and_then(|b| b.build().ok());
    for step in Autodiscover::for_domain(&domain) {
        match step {
            Autodiscover::Srv { name, .. } => {
                let Some(r) = resolver.as_ref() else { continue };
                let Ok(answer) = r
                    .lookup(name.as_str(), hickory_resolver::proto::rr::RecordType::SRV)
                    .await
                else {
                    continue;
                };
                for record in answer.answers() {
                    let hickory_resolver::proto::rr::RData::SRV(rec) = &record.data else {
                        continue;
                    };
                    let target = rec.target.to_utf8();
                    let Some(e) = mailrs_mailprovider::from_srv(&name, &target, rec.port) else {
                        continue;
                    };
                    let slot = match e.protocol {
                        mailrs_mailprovider::Protocol::Smtp => &mut row.outgoing,
                        _ => &mut row.incoming,
                    };
                    // First answer wins: SRV priority is the provider's
                    // own ordering and the resolver hands them back in
                    // it.
                    if slot.host.trim().is_empty() {
                        *slot = endpoint_of(&e);
                    }
                }
            }
            // The community database needs an HTTP fetch, which this
            // does not do — a lookup on the set-up path should not wait
            // on somebody else's web server. The guess below covers the
            // same ground for the servers that follow convention.
            Autodiscover::Ispdb { .. } => {}
            Autodiscover::Guess { imap, smtp } => {
                if row.incoming.host.trim().is_empty() {
                    row.incoming = endpoint_of(&imap);
                }
                if row.outgoing.host.trim().is_empty() {
                    row.outgoing = endpoint_of(&smtp);
                }
            }
        }
    }
}
