//! JMAP over WebSocket (RFC 8887): requests and their responses on one
//! connection, and push (`WebSocketPushEnable`) without EventSource.
//!
//! The upgrade passes the same login as `/jmap` (session, agent key, or an
//! app password over HTTP Basic). As with the web client's own socket, a
//! connection a browser opened with the session cookie must come from one
//! of our pages (its `Origin`); a Bearer token is never sent by a browser
//! on its own and is taken as it is. Basic credentials a browser may have
//! cached are refused from another site's page as well.

use std::sync::Arc;

use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio::time::MissedTickBehavior;

use super::ctx::Ctx;
use super::limits::Slot;
use super::push;
use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::session::csrf;

pub const WEBSOCKET_CAP: &str = "urn:ietf:params:jmap:websocket";

/// How the upgrade was signed in, as far as the Origin rule cares.
#[derive(Debug, PartialEq, Eq)]
enum Login {
    Bearer,
    Basic,
    Cookie,
}

fn login_of(headers: &HeaderMap) -> Login {
    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let scheme = auth.split_whitespace().next().unwrap_or("");
    match scheme.to_ascii_lowercase().as_str() {
        "bearer" => Login::Bearer,
        "basic" => Login::Basic,
        _ => Login::Cookie,
    }
}

/// Whether the upgrade may proceed from the page (if any) that asked.
fn origin_allowed(headers: &HeaderMap, origins: &[String]) -> bool {
    let ours = csrf::origin_ok(headers, origins);
    match login_of(headers) {
        Login::Bearer => true,
        Login::Basic => ours != Some(false),
        Login::Cookie => ours == Some(true),
    }
}

/// RFC 8887 §4.2: the client asks for the `jmap` subprotocol.
fn asks_for_jmap(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::SEC_WEBSOCKET_PROTOCOL)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .any(|p| p.trim().eq_ignore_ascii_case("jmap"))
}

/// The URL the session document gives for this transport.
pub fn url(base: &str) -> String {
    let ws = match base.strip_prefix("https://") {
        Some(rest) => format!("wss://{rest}"),
        None => match base.strip_prefix("http://") {
            Some(rest) => format!("ws://{rest}"),
            None => base.to_string(),
        },
    };
    format!("{ws}/jmap/ws/")
}

/// GET /jmap/ws/
pub async fn websocket(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    scopes: Option<Extension<crate::session::AgentScopes>>,
    key: Option<Extension<crate::session::AgentKey>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !origin_allowed(&headers, &csrf::config().origins) {
        return (StatusCode::FORBIDDEN, "not from this site").into_response();
    }
    if !asks_for_jmap(&headers) {
        return (StatusCode::BAD_REQUEST, "the jmap subprotocol is required").into_response();
    }
    let Some(slot) = Slot::take_push(&user, push::MAX_CONNECTIONS) else {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "too many push connections for this account",
        )
            .into_response();
    };
    let masked_only = scopes
        .as_ref()
        .is_some_and(|Extension(s)| crate::session::masked_only(&s.0));
    let key = key.map(|Extension(k)| k);
    ws.protocols(["jmap"]).on_upgrade(move |socket| async move {
        let bus = crate::handlers::events::get_or_init_bus(state.clone())
            .await
            .subscribe();
        let conn = Conn {
            ctx: Ctx::new(state.clone(), user, key.clone()),
            state,
            masked_only,
            key,
            push: None,
            _slot: slot,
        };
        conn.serve(socket, bus).await;
    })
}

struct Push {
    types: Vec<&'static str>,
    last: Option<String>,
}

struct Conn {
    ctx: Ctx,
    state: Arc<WebState>,
    masked_only: bool,
    key: Option<crate::session::AgentKey>,
    push: Option<Push>,
    _slot: Slot,
}

fn text(v: Value) -> Message {
    Message::Text(v.to_string().into())
}

fn request_error(id: &Value, kind: &str, detail: &str, limit: Option<&str>) -> Value {
    let mut e = json!({ "@type": "RequestError", "type": kind, "status": 400, "detail": detail });
    if !id.is_null() {
        e["requestId"] = id.clone();
    }
    if let Some(l) = limit {
        e["limit"] = json!(l);
    }
    e
}

impl Conn {
    async fn serve(mut self, socket: WebSocket, mut bus: broadcast::Receiver<String>) {
        let (mut sink, mut stream) = socket.split();
        let mut check = tokio::time::interval(push::CHECK);
        check.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            let pushing = self.push.is_some();
            let out = tokio::select! {
                msg = stream.next() => match msg {
                    Some(Ok(Message::Text(t))) => self.answer(t.as_bytes()).await,
                    Some(Ok(Message::Binary(_))) => {
                        let close = CloseFrame { code: 1003, reason: "JMAP is sent as text".into() };
                        let _ = sink.send(Message::Close(Some(close))).await;
                        return;
                    }
                    Some(Ok(_)) => Vec::new(),
                    Some(Err(_)) | None => return,
                },
                _ = check.tick(), if pushing => self.state_change(false).await,
                m = bus.recv(), if pushing => match m {
                    Ok(t) if crate::handlers::events::addressed_to(&t, &self.ctx.user) => self.state_change(false).await,
                    Ok(_) => Vec::new(),
                    Err(broadcast::error::RecvError::Lagged(_)) => self.state_change(false).await,
                    Err(broadcast::error::RecvError::Closed) => Vec::new(),
                },
            };
            for v in out {
                if sink.send(text(v)).await.is_err() {
                    return;
                }
            }
        }
    }

    /// What to send for one text frame from the client.
    async fn answer(&mut self, frame: &[u8]) -> Vec<Value> {
        let Ok(v) = serde_json::from_slice::<Value>(frame) else {
            return vec![request_error(
                &Value::Null,
                "urn:ietf:params:jmap:error:notJSON",
                "not JSON",
                None,
            )];
        };
        let id = v.get("id").cloned().unwrap_or(Value::Null);
        match v.get("@type").and_then(|t| t.as_str()) {
            Some("Request") => {
                let r = super::process(
                    &self.state,
                    &self.ctx.user,
                    self.masked_only,
                    self.key.clone(),
                    frame,
                )
                .await;
                match r {
                    Ok(mut resp) => {
                        resp["@type"] = json!("Response");
                        if !id.is_null() {
                            resp["requestId"] = id;
                        }
                        vec![resp]
                    }
                    Err(p) => vec![request_error(&id, p.kind, &p.detail, p.limit)],
                }
            }
            Some("WebSocketPushEnable") => {
                let types = match v.get("dataTypes").and_then(|d| d.as_array()) {
                    None => push::types_of(None),
                    Some(list) => {
                        let names: Vec<&str> = list.iter().filter_map(|t| t.as_str()).collect();
                        push::types_of(Some(&names.join(",")))
                    }
                };
                let since = v
                    .get("pushState")
                    .and_then(|s| s.as_str())
                    .map(str::to_string);
                self.push = Some(Push {
                    types,
                    last: since.clone(),
                });
                // a client that says where it was hears at once if it is behind
                self.state_change(since.is_none()).await
            }
            Some("WebSocketPushDisable") => {
                self.push = None;
                Vec::new()
            }
            _ => vec![request_error(
                &id,
                "urn:ietf:params:jmap:error:notRequest",
                "unknown @type",
                None,
            )],
        }
    }

    /// A StateChange when the state moved since the last one; `quiet`
    /// only records where it is.
    async fn state_change(&mut self, quiet: bool) -> Vec<Value> {
        let Some(now) = push::read_state(&self.ctx).await else {
            return Vec::new();
        };
        let Some(p) = self.push.as_mut() else {
            return Vec::new();
        };
        let moved = p.last.as_ref().is_some_and(|l| *l != now);
        p.last = Some(now.clone());
        if quiet || !moved {
            return Vec::new();
        }
        let changed: serde_json::Map<String, Value> = p
            .types
            .iter()
            .map(|t| (t.to_string(), json!(now)))
            .collect();
        vec![json!({
            "@type": "StateChange",
            "changed": { &self.ctx.user: changed },
            "pushState": now,
        })]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(*k, v.parse().unwrap());
        }
        h
    }

    #[test]
    fn a_cookie_upgrade_must_come_from_our_page_and_a_token_need_not() {
        let ours = vec!["https://mail.example.test".to_string()];
        let ok = |pairs: &[(&'static str, &str)]| origin_allowed(&headers(pairs), &ours);
        assert!(ok(&[("origin", "https://mail.example.test")]));
        assert!(!ok(&[("origin", "https://evil.example")]));
        assert!(!ok(&[]), "a cookie with no origin at all");
        assert!(ok(&[
            ("authorization", "Bearer x"),
            ("origin", "https://evil.example")
        ]));
        assert!(ok(&[("authorization", "Basic eDp5")]));
        assert!(!ok(&[
            ("authorization", "Basic eDp5"),
            ("origin", "https://evil.example")
        ]));
    }

    #[test]
    fn the_subprotocol_is_required_and_the_url_follows_the_scheme() {
        assert!(asks_for_jmap(&headers(&[(
            "sec-websocket-protocol",
            "chat, jmap"
        )])));
        assert!(!asks_for_jmap(&headers(&[(
            "sec-websocket-protocol",
            "chat"
        )])));
        assert_eq!(
            url("https://mail.example.test"),
            "wss://mail.example.test/jmap/ws/"
        );
        assert_eq!(url("http://127.0.0.1:9"), "ws://127.0.0.1:9/jmap/ws/");
    }
}
