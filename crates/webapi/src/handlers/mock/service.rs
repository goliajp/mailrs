use std::sync::Arc;

use axum::http::StatusCode;
use mailrs_core_sidestate::families::mock::{self, MockError, Namespace};
use serde::Serialize;

use crate::WebState;
use crate::handlers::kevy_util::{require_admin, with_kevy};

/// Who is asking, as far as mock mail is concerned.
#[derive(Debug, Clone)]
pub struct MockCaller {
    pub user: String,
    /// a key with `mock:{ns}` scopes reaches only those namespaces
    pub only_namespaces: Option<Vec<String>>,
}

#[derive(Debug)]
pub struct MockApiError {
    pub status: StatusCode,
    pub message: String,
}

impl MockApiError {
    pub(super) fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl From<StatusCode> for MockApiError {
    fn from(status: StatusCode) -> Self {
        let message = status.canonical_reason().unwrap_or("error").to_string();
        Self { status, message }
    }
}

impl From<MockError> for MockApiError {
    fn from(e: MockError) -> Self {
        let status = match e {
            MockError::BadName(_) => StatusCode::BAD_REQUEST,
            MockError::Exists => StatusCode::CONFLICT,
            MockError::NotFound => StatusCode::NOT_FOUND,
            MockError::TooManyNamespaces | MockError::Disabled | MockError::AdminsOnly => {
                StatusCode::FORBIDDEN
            }
            MockError::BadSetting(_) => StatusCode::BAD_REQUEST,
            MockError::StoreFull => StatusCode::INSUFFICIENT_STORAGE,
            MockError::Kevy(_) | MockError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self::new(status, e.to_string())
    }
}

pub type MockApiResult<T> = Result<T, MockApiError>;

pub(super) fn store<T>(
    f: impl FnOnce(&mut kevy_client::Connection) -> mock::MockResult<T>,
) -> MockApiResult<T> {
    let mut inner = None;
    with_kevy(|conn| {
        inner = Some(f(conn));
        Ok(())
    })?;
    inner.expect("the closure ran").map_err(Into::into)
}

fn enabled() -> MockApiResult<&'static str> {
    match mock::domain() {
        "" => Err(MockApiError::new(
            StatusCode::NOT_FOUND,
            "mock mail is not enabled on this deployment",
        )),
        d => Ok(d),
    }
}

async fn is_admin(state: &Arc<WebState>, user: &str) -> bool {
    require_admin(state, user).await.is_ok()
}

/// The namespace, when `caller` may read and change it. A namespace the
/// caller may not see answers as missing.
pub(super) async fn authorize(
    state: &Arc<WebState>,
    caller: &MockCaller,
    ns: &str,
) -> MockApiResult<Namespace> {
    enabled()?;
    let missing = || MockApiError::new(StatusCode::NOT_FOUND, "no such namespace");
    if caller
        .only_namespaces
        .as_ref()
        .is_some_and(|only| !only.iter().any(|n| n == ns))
    {
        return Err(missing());
    }
    let ns_name = ns.to_string();
    let user = caller.user.clone();
    let admin = is_admin(state, &caller.user).await;
    store(move |c| mock::namespace_for(c, &ns_name, &user, admin))?.ok_or_else(missing)
}

#[derive(Debug, Serialize)]
pub struct NamespaceView {
    #[serde(flatten)]
    pub namespace: Namespace,
    /// an example address; any tag works
    pub address: String,
    /// bytes of the namespace's message files
    pub bytes: i64,
}

fn view(namespace: Namespace) -> NamespaceView {
    let address = format!("{}.test@{}", namespace.name, mock::domain());
    let bytes = mock::namespace_bytes(mock::root(), &namespace.name);
    NamespaceView {
        namespace,
        address,
        bytes,
    }
}

pub async fn create_namespace(
    state: &Arc<WebState>,
    caller: &MockCaller,
    name: &str,
) -> MockApiResult<NamespaceView> {
    enabled()?;
    let policy = store(mock::settings)?.create;
    if policy == mock::CreatePolicy::AdminsOnly && !is_admin(state, &caller.user).await {
        return Err(MockError::AdminsOnly.into());
    }
    if caller.only_namespaces.is_some() {
        return Err(MockApiError::new(
            StatusCode::FORBIDDEN,
            "a key scoped to namespaces cannot create namespaces",
        ));
    }
    let (user, name) = (caller.user.clone(), name.trim().to_ascii_lowercase());
    let created = store(move |c| mock::create_namespace(c, &user, &name))?;
    crate::handlers::audit::record(&caller.user, "mock.namespace_create", &created.name, "");
    Ok(view(created))
}

/// The caller's namespaces; every namespace when an admin asks for `all`.
pub async fn list_namespaces(
    state: &Arc<WebState>,
    caller: &MockCaller,
    all: bool,
) -> MockApiResult<Vec<NamespaceView>> {
    enabled()?;
    if let Some(only) = &caller.only_namespaces {
        let mut out = Vec::new();
        for ns in only {
            if let Ok(found) = authorize(state, caller, ns).await {
                out.push(view(found));
            }
        }
        return Ok(out);
    }
    let owner = match all && is_admin(state, &caller.user).await {
        true => None,
        false => Some(caller.user.clone()),
    };
    let list = store(move |c| mock::list_namespaces(c, owner.as_deref()))?;
    Ok(list.into_iter().map(view).collect())
}

pub async fn delete_namespace(
    state: &Arc<WebState>,
    caller: &MockCaller,
    ns: &str,
) -> MockApiResult<()> {
    if caller.only_namespaces.is_some() {
        return Err(MockApiError::new(
            StatusCode::FORBIDDEN,
            "a key scoped to namespaces cannot delete them",
        ));
    }
    let found = authorize(state, caller, ns).await?;
    let ns = ns.to_string();
    store(move |c| mock::delete_namespace(c, mock::root(), &ns))?;
    let detail = format!("owner={}", found.owner);
    crate::handlers::audit::record(&caller.user, "mock.namespace_delete", &found.name, &detail);
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct MintedAddress {
    pub address: String,
    pub namespace: String,
    pub tag: String,
    pub minted_at: i64,
}

pub async fn new_address(
    state: &Arc<WebState>,
    caller: &MockCaller,
    ns: &str,
    tag: Option<&str>,
) -> MockApiResult<MintedAddress> {
    authorize(state, caller, ns).await?;
    let tag = match tag
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
    {
        Some(t) if mailrs_mock::address::validate_tag(&t) => t,
        Some(_) => {
            return Err(MockApiError::new(
                StatusCode::BAD_REQUEST,
                "a tag is up to 31 characters of a-z, 0-9, '.', '_' and '-'",
            ));
        }
        None => random_tag(),
    };
    let minted_at = now_ms();
    let (ns_name, t) = (ns.to_string(), tag.clone());
    store(move |c| mock::record_minted(c, &ns_name, &t, minted_at))?;
    Ok(MintedAddress {
        address: format!("{ns}.{tag}@{}", mock::domain()),
        namespace: ns.to_string(),
        tag,
        minted_at,
    })
}

fn random_tag() -> String {
    let mut bytes = [0u8; 5];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(super) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Serialize)]
pub struct MockWebhook {
    pub url: String,
    /// HMAC key the deliveries are signed with, as for every webhook
    pub signing_secret: String,
}

/// POST each new message of `ns` to `url`, replacing any earlier webhook.
pub async fn set_webhook(
    state: &Arc<WebState>,
    caller: &MockCaller,
    ns: &str,
    url: &str,
) -> MockApiResult<MockWebhook> {
    authorize(state, caller, ns).await?;
    let url = url.trim().to_string();
    let scheme_ok = url.starts_with("https://") || url.starts_with("http://");
    if !scheme_ok || url.len() > 2048 || url.contains(char::is_whitespace) {
        return Err(MockApiError::new(
            StatusCode::BAD_REQUEST,
            "the webhook is an http(s) URL",
        ));
    }
    let ns_name = ns.to_string();
    let sub = store(move |c| mock::set_webhook(c, &ns_name, &url))?;
    Ok(MockWebhook {
        url: sub.url,
        signing_secret: sub.signing_secret,
    })
}

pub async fn clear_webhook(
    state: &Arc<WebState>,
    caller: &MockCaller,
    ns: &str,
) -> MockApiResult<bool> {
    authorize(state, caller, ns).await?;
    let ns_name = ns.to_string();
    store(move |c| mock::clear_webhook(c, &ns_name))
}
