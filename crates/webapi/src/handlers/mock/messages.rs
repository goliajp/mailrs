//! Reading, waiting for and deleting a namespace's messages, and its
//! sandbox SMTP password.

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use mailrs_core_sidestate::families::mock::{self, Filter, MessageMeta};
use serde::Serialize;

use super::service::{MockApiError, MockApiResult, MockCaller, authorize, now_ms, store};
use crate::WebState;

#[derive(Debug, Serialize)]
pub struct MessageList {
    pub messages: Vec<MessageMeta>,
    /// messages this namespace lost to its cap
    pub dropped: i64,
}

pub async fn list_messages(
    state: &Arc<WebState>,
    caller: &MockCaller,
    ns: &str,
    filter: Filter,
) -> MockApiResult<MessageList> {
    let found = authorize(state, caller, ns).await?;
    let ns = ns.to_string();
    let messages = store(move |c| mock::list_messages(c, &ns, &filter))?;
    Ok(MessageList {
        messages,
        dropped: found.dropped,
    })
}

#[derive(Debug, Serialize)]
pub struct FullMessage {
    #[serde(flatten)]
    pub meta: MessageMeta,
    pub text: String,
    pub html: Option<String>,
    pub dropped: i64,
}

fn full(meta: MessageMeta, dropped: i64) -> MockApiResult<FullMessage> {
    let raw = mock::read_raw(mock::root(), &meta.namespace, &meta.id)
        .map_err(|_| MockApiError::new(StatusCode::NOT_FOUND, "the message file is gone"))?;
    let parsed = mailrs_mock::parse_message(&raw);
    Ok(FullMessage {
        meta,
        text: parsed.text,
        html: parsed.html,
        dropped,
    })
}

pub async fn read_message(
    state: &Arc<WebState>,
    caller: &MockCaller,
    ns: &str,
    id: &str,
) -> MockApiResult<FullMessage> {
    let found = authorize(state, caller, ns).await?;
    let (ns, id) = (ns.to_string(), id.to_string());
    let meta = store(move |c| mock::get_message(c, &ns, &id))?
        .ok_or_else(|| MockApiError::new(StatusCode::NOT_FOUND, "no such message"))?;
    full(meta, found.dropped)
}

pub async fn read_raw(
    state: &Arc<WebState>,
    caller: &MockCaller,
    ns: &str,
    id: &str,
) -> MockApiResult<Vec<u8>> {
    authorize(state, caller, ns).await?;
    let (ns_name, id_s) = (ns.to_string(), id.to_string());
    store(move |c| mock::get_message(c, &ns_name, &id_s))?
        .ok_or_else(|| MockApiError::new(StatusCode::NOT_FOUND, "no such message"))?;
    mock::read_raw(mock::root(), ns, id)
        .map_err(|_| MockApiError::new(StatusCode::NOT_FOUND, "the message file is gone"))
}

/// The newest message matching `filter`, if any.
pub async fn latest(
    state: &Arc<WebState>,
    caller: &MockCaller,
    ns: &str,
    mut filter: Filter,
) -> MockApiResult<Option<FullMessage>> {
    let found = authorize(state, caller, ns).await?;
    filter.limit = 1;
    let ns = ns.to_string();
    let newest = store(move |c| mock::list_messages(c, &ns, &filter))?;
    newest
        .into_iter()
        .next()
        .map(|m| full(m, found.dropped))
        .transpose()
}

pub const DEFAULT_WAIT: Duration = Duration::from_secs(30);
pub const MAX_WAIT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(500);

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum WaitOutcome {
    Received {
        #[serde(flatten)]
        message: Box<FullMessage>,
    },
    Timeout {
        dropped: i64,
    },
}

/// Wait for the first message matching `filter` that arrived at or after
/// `filter.since`. Without `since`, that is from when `filter.tag` was
/// handed out, or from now.
pub async fn wait_for_message(
    state: &Arc<WebState>,
    caller: &MockCaller,
    ns: &str,
    mut filter: Filter,
    timeout: Option<Duration>,
) -> MockApiResult<WaitOutcome> {
    let found = authorize(state, caller, ns).await?;
    if filter.since.is_none() {
        let minted = match filter.tag.clone() {
            Some(tag) => {
                let ns = ns.to_string();
                store(move |c| mock::minted_at(c, &ns, &tag))?
            }
            None => None,
        };
        filter.since = Some(minted.unwrap_or_else(now_ms));
    }
    filter.limit = usize::MAX;
    let deadline = tokio::time::Instant::now() + timeout.unwrap_or(DEFAULT_WAIT).min(MAX_WAIT);
    loop {
        let (ns_name, f) = (ns.to_string(), filter.clone());
        // oldest first: the first message after `since` is the one being waited for
        if let Some(m) = store(move |c| mock::list_messages(c, &ns_name, &f))?.pop() {
            return Ok(WaitOutcome::Received {
                message: Box::new(full(m, found.dropped)?),
            });
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(WaitOutcome::Timeout {
                dropped: found.dropped,
            });
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Delete one message, or with `id` absent every message (with `tag`).
/// Returns how many were removed.
pub async fn delete_messages(
    state: &Arc<WebState>,
    caller: &MockCaller,
    ns: &str,
    id: Option<&str>,
    tag: Option<&str>,
) -> MockApiResult<usize> {
    authorize(state, caller, ns).await?;
    let (ns, id, tag) = (
        ns.to_string(),
        id.map(str::to_string),
        tag.map(str::to_string),
    );
    store(move |c| match id {
        Some(id) => mock::delete_message(c, mock::root(), &ns, &id).map(usize::from),
        None => mock::delete_messages(c, mock::root(), &ns, tag.as_deref()),
    })
}

#[derive(Debug, Serialize)]
pub struct SmtpCredentials {
    /// submission on 587 (STARTTLS) or 465 (TLS)
    pub host: String,
    pub username: String,
    /// shown once; a new one replaces it
    pub password: String,
}

/// Issue a new sandbox SMTP password for `ns`, replacing any earlier one.
pub async fn rotate_smtp_token(
    state: &Arc<WebState>,
    caller: &MockCaller,
    ns: &str,
) -> MockApiResult<SmtpCredentials> {
    authorize(state, caller, ns).await?;
    let ns_name = ns.to_string();
    let password = store(move |c| mock::rotate_smtp_token(c, &ns_name))?;
    Ok(SmtpCredentials {
        host: std::env::var("MAILRS_HOSTNAME").unwrap_or_default(),
        username: format!("{ns}@{}", mock::domain()),
        password,
    })
}
