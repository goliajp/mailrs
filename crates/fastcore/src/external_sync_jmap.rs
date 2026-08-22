//! The JMAP arm of the connected-mailbox sync.
//!
//! Split from `external_sync.rs` when the three protocols together
//! passed the 500-line limit. They divide cleanly, because the only
//! thing they share is where they end — `ingest_delivered_file`, the
//! same door the spool uses — and what each remembers between syncs is
//! the whole of their difference.

use std::sync::Arc;

use mailrs_core_sidestate::families::external_accounts::AccountRow;
use std::time::Duration;

use super::external_sync::{now_secs, sanitise};
use crate::FastcoreState;
use mailrs_jmap_client as jmap;

/// Read one JMAP account.
///
/// Three requests: the session object for where the API actually is,
/// `Email/changes` (or `Email/query` the first time) for what to
/// fetch, and the download URL for each message's bytes.
///
/// **The whole message is downloaded as a blob**, not reassembled from
/// the parts `Email/get` returns. A reassembled message is one that
/// never existed: every signature over it fails, and DKIM on a
/// forwarded copy is exactly what somebody would notice.
pub(crate) async fn sync_jmap(
    state: &Arc<FastcoreState>,
    user: &str,
    row: &AccountRow,
    secret: &str,
) -> Result<usize, String> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    // Where the API is, from the server rather than from what somebody
    // typed — a provider that moves its endpoint would otherwise break
    // every account added before the move.
    let well_known = format!("https://{}/.well-known/jmap", row.incoming.host);
    let body = http
        .get(&well_known)
        .bearer_auth(secret)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let session = jmap::parse_session(&body).ok_or("this server does not offer JMAP for mail")?;

    let held_state = read_jmap_marker(state, &row.id);
    let (ids, new_state) = match held_state.as_deref() {
        // Never synced: ask what is there rather than what changed.
        None | Some("") => query_recent(&http, &session, secret).await?,
        Some(since) => match changes_since(&http, &session, secret, since).await? {
            // The server has forgotten that state. Reading the mailbox
            // again is the only correct answer — carrying on would mean
            // never seeing another message, and nothing about that
            // looks like a failure from outside.
            jmap::Changes::StartOver => {
                tracing::info!(
                    %user, account = %row.email,
                    "the server can no longer say what changed — reading the mailbox again"
                );
                query_recent(&http, &session, secret).await?
            }
            jmap::Changes::Moved {
                created,
                new_state,
                has_more,
                ..
            } => {
                if has_more {
                    // Said out loud: the next tick continues, and a
                    // sync that looks short is otherwise indistinguish-
                    // able from one that finished.
                    tracing::info!(%user, account = %row.email, "more changes pending");
                }
                (created, new_state)
            }
        },
    };

    let mut filed = 0usize;
    for id in &ids {
        let url = jmap::blob_url(&session, id, "message.eml");
        let Ok(resp) = http.get(&url).bearer_auth(secret).send().await else {
            continue;
        };
        let Ok(bytes) = resp.bytes().await else {
            continue;
        };
        let blob = format!("ext-{}-jmap-{}", row.id, sanitise(id));
        crate::ingest::ingest_delivered_file(state, user, &blob, &bytes, "INBOX");
        filed += 1;
    }
    write_jmap_marker(state, &row.id, &new_state);
    Ok(filed)
}

/// The JMAP state this account last synced from.
///
/// One opaque string, and the server's to interpret — the client never
/// parses it, compares it, or invents one. `None` means never synced,
/// which is a different question from "synced and nothing changed" and
/// takes a different first request.
fn read_jmap_marker(state: &Arc<FastcoreState>, account_id: &str) -> Option<String> {
    let mut conn = state.net_conn()?;
    let key = format!("ext:sync:{account_id}:jmap");
    let raw = conn.hget(key.as_bytes(), b"state").ok()??;
    let s = String::from_utf8(raw).ok()?;
    if s.is_empty() { None } else { Some(s) }
}

fn write_jmap_marker(state: &Arc<FastcoreState>, account_id: &str, new_state: &str) {
    let Some(mut conn) = state.net_conn() else {
        return;
    };
    let key = format!("ext:sync:{account_id}:jmap");
    let at = now_secs().to_string();
    let _ = conn.hset(
        key.as_bytes(),
        &[
            (b"state".as_slice(), new_state.as_bytes()),
            (b"at".as_slice(), at.as_bytes()),
        ],
    );
}

/// Everything in the mailbox, newest first, for a first sync or after
/// the server has forgotten our state.
///
/// Bounded, because "everything" on a ten-year mailbox is not a first
/// impression anybody wants to wait for — the rest arrives on later
/// ticks, oldest last, which is the order somebody reading their mail
/// would choose.
const JMAP_FIRST_PAGE: usize = 500;

/// The newest messages in a JMAP mailbox, for a first sync or after
/// the server has forgotten our state.
async fn query_recent(
    http: &reqwest::Client,
    session: &jmap::Session,
    token: &str,
) -> Result<(Vec<String>, String), String> {
    let body = serde_json::json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
        "methodCalls": [
            ["Email/query", {
                "accountId": session.account_id,
                "sort": [{"property": "receivedAt", "isAscending": false}],
                "limit": JMAP_FIRST_PAGE,
            }, "q"],
            ["Email/get", {
                "accountId": session.account_id,
                "#ids": {"resultOf": "q", "name": "Email/query", "path": "/ids"},
                "properties": ["id", "blobId"],
            }, "g"],
        ]
    });
    let v = post_jmap(http, session, token, &body).await?;
    let ids = v["methodResponses"]
        .as_array()
        .and_then(|calls| calls.iter().find(|c| c[0] == "Email/query"))
        .and_then(|c| c[1]["ids"].as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    // The state to ask changes from next time comes from the query
    // itself: asking for changes from a state we never held would be
    // asking the server about somebody else's history.
    let state = v["methodResponses"]
        .as_array()
        .and_then(|calls| calls.iter().find(|c| c[0] == "Email/query"))
        .and_then(|c| c[1]["queryState"].as_str())
        .unwrap_or_default()
        .to_string();
    Ok((ids, state))
}

/// What changed since a state the server still remembers.
async fn changes_since(
    http: &reqwest::Client,
    session: &jmap::Session,
    token: &str,
    since: &str,
) -> Result<jmap::Changes, String> {
    let body = serde_json::json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
        "methodCalls": [
            ["Email/changes", {"accountId": session.account_id, "sinceState": since}, "c"]
        ]
    });
    let v = post_jmap(http, session, token, &body).await?;
    jmap::parse_changes(&v.to_string()).ok_or_else(|| "unreadable Email/changes answer".to_string())
}

async fn post_jmap(
    http: &reqwest::Client,
    session: &jmap::Session,
    token: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let text = http
        .post(&session.api_url)
        .bearer_auth(token)
        .json(body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}
