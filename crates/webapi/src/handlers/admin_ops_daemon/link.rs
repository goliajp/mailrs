//! Calling the ops daemon's administrator routes, shared by the REST
//! routes and the MCP tools.
//!
//! webapi calls the daemon at `MAILRS_OPS_URL` with the shared secret
//! `MAILRS_OPS_ADMIN_SECRET` (the daemon keeps only its sha256) and names
//! the administrator, which the daemon records with every change. The
//! secret goes in one request header and nowhere else: not in an answer,
//! not in a log line, not in the audit log.

use std::sync::OnceLock;
use std::time::Duration;

use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};

const ADMIN_HEADER: &str = "x-mailrs-ops-admin";
const ACTOR_HEADER: &str = "x-mailrs-ops-actor";
const TIMEOUT: Duration = Duration::from_secs(5);

/// Where the daemon is and the secret that opens its administrator routes.
struct Link {
    base: String,
    secret: String,
}

fn link() -> Option<Link> {
    let var = |n: &str| std::env::var(n).ok().filter(|v| !v.trim().is_empty());
    Some(Link {
        base: var("MAILRS_OPS_URL")?.trim_end_matches('/').to_string(),
        secret: var("MAILRS_OPS_ADMIN_SECRET")?,
    })
}

fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

/// How a call to the daemon went.
enum Reply {
    NotConfigured,
    Unreachable(String),
    /// The daemon refused webapi's secret (401 or 403).
    Refused,
    /// The daemon answered with this status and body.
    Answered(StatusCode, Value),
}

async fn call(method: reqwest::Method, path: &str, actor: &str, body: Option<Value>) -> Reply {
    let Some(link) = link() else {
        return Reply::NotConfigured;
    };
    let mut req = http()
        .request(method, format!("{}{path}", link.base))
        .header(ADMIN_HEADER, &link.secret)
        .header(ACTOR_HEADER, actor)
        .timeout(TIMEOUT);
    if let Some(b) = body {
        req = req.json(&b);
    }
    let res = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            // the error and its causes, which name no header
            let e = e.without_url();
            let mut why = e.to_string();
            let mut cause = std::error::Error::source(&e);
            while let Some(c) = cause {
                why.push_str(": ");
                why.push_str(&c.to_string());
                cause = c.source();
            }
            tracing::warn!(error = %why, "ops daemon not reached");
            return Reply::Unreachable(why);
        }
    };
    let status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        tracing::error!(%status, "ops daemon refused webapi's admin secret");
        return Reply::Refused;
    }
    let text = res.text().await.unwrap_or_default();
    // the daemon answers JSON on success and plain text on a refusal
    let value = serde_json::from_str(&text).unwrap_or(Value::String(text));
    Reply::Answered(status, value)
}

/// Why a request was not carried out, with the status REST answers.
#[derive(Debug)]
pub struct Refusal {
    pub status: StatusCode,
    pub message: String,
}

fn refuse(status: StatusCode, message: impl Into<String>) -> Refusal {
    Refusal {
        status,
        message: message.into(),
    }
}

/// The daemon's answer on success, its own reason on a request it turned
/// down, 502 when it could not be asked.
fn passed_on(reply: Reply) -> Result<Value, Refusal> {
    match reply {
        Reply::NotConfigured => Err(refuse(
            StatusCode::BAD_GATEWAY,
            "the ops daemon is not configured: MAILRS_OPS_URL or MAILRS_OPS_ADMIN_SECRET is not set",
        )),
        Reply::Unreachable(why) => Err(refuse(
            StatusCode::BAD_GATEWAY,
            format!("webapi cannot reach the ops daemon: {why}"),
        )),
        Reply::Refused => Err(refuse(
            StatusCode::BAD_GATEWAY,
            "the ops daemon refused webapi's admin secret",
        )),
        Reply::Answered(s, v) if s.is_success() => Ok(v),
        Reply::Answered(s, v) => {
            let why = match v {
                Value::String(t) => t,
                other => other.to_string(),
            };
            let status = match s {
                StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND | StatusCode::CONFLICT => s,
                _ => StatusCode::BAD_GATEWAY,
            };
            Err(refuse(status, why))
        }
    }
}

/// How long the daemon may go without completing a cycle before it counts
/// as silent: `MAILRS_OPS_SILENT_SECS`, 600 by default.
pub fn silent_after_secs() -> i64 {
    std::env::var("MAILRS_OPS_SILENT_SECS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(600)
}

/// The daemon's state, or why it is not there: `reach` is `ok`,
/// `not_configured`, `unreachable` or `refused`, so "no alerts" and
/// "cannot see the daemon" never look the same.
pub async fn read_state(actor: &str) -> Value {
    let limit = silent_after_secs();
    match call(reqwest::Method::GET, "/v1/state", actor, None).await {
        Reply::NotConfigured => json!({ "reach": "not_configured" }),
        Reply::Unreachable(why) => json!({ "reach": "unreachable", "detail": why }),
        Reply::Refused => json!({ "reach": "refused" }),
        Reply::Answered(s, v) if s.is_success() => {
            let now = crate::handlers::complete::now_secs();
            json!({
                "reach": "ok",
                "silent_after_secs": limit,
                "stale": crate::ops_watch::silent(Some(&v), now, limit),
                "state": v,
            })
        }
        Reply::Answered(s, _) => json!({
            "reach": "unreachable",
            "detail": format!("the ops daemon answered {s}"),
        }),
    }
}

/// Alert transitions, notifications and silence / maintenance events since
/// `since` (the daemon's default: the last day).
pub async fn read_history(
    actor: &str,
    since: Option<i64>,
    limit: Option<u32>,
) -> Result<Value, Refusal> {
    let mut path = format!("/v1/history?limit={}", limit.unwrap_or(200).min(1000));
    if let Some(s) = since {
        path.push_str(&format!("&since={s}"));
    }
    passed_on(call(reqwest::Method::GET, &path, actor, None).await)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SilenceRequest {
    pub rule: String,
    /// One object of the rule; every object when absent.
    pub object: Option<String>,
    pub for_secs: i64,
    pub reason: String,
}

/// Silence one rule (or one object of it) for `for_secs`, for a reason.
pub async fn silence(actor: &str, req: &SilenceRequest) -> Result<Value, Refusal> {
    if req.for_secs <= 0 {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "a silence needs an end in the future",
        ));
    }
    let body = json!({
        "rule": req.rule, "object": req.object, "for_secs": req.for_secs, "reason": req.reason,
    });
    let answer = passed_on(call(reqwest::Method::POST, "/v1/silences", actor, Some(body)).await)?;
    let target = format!("{}/{}", req.rule, req.object.as_deref().unwrap_or("*"));
    let detail = format!("for {}s: {}", req.for_secs, req.reason);
    crate::handlers::audit::record(actor, "ops.silence.set", &target, &detail);
    Ok(answer)
}

fn encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// End silence `id` before its time, for a reason.
pub async fn unsilence(actor: &str, id: &str, reason: &str) -> Result<Value, Refusal> {
    if reason.trim().is_empty() {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "ending a silence needs a reason",
        ));
    }
    let path = format!("/v1/silences/{}?reason={}", encode(id), encode(reason));
    let answer = passed_on(call(reqwest::Method::DELETE, &path, actor, None).await)?;
    crate::handlers::audit::record(actor, "ops.silence.end", id, reason);
    Ok(answer)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceRequest {
    pub action: MaintenanceAction,
    /// How long from now it lasts; required to start or extend.
    pub for_secs: Option<i64>,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MaintenanceAction {
    Start,
    Extend,
    End,
}

impl MaintenanceAction {
    pub fn as_str(self) -> &'static str {
        match self {
            MaintenanceAction::Start => "start",
            MaintenanceAction::Extend => "extend",
            MaintenanceAction::End => "end",
        }
    }
}

/// Start, extend or end maintenance. Starting and extending need an end:
/// the daemon's two-hour default is for its own command line.
pub async fn change_maintenance(actor: &str, req: &MaintenanceRequest) -> Result<Value, Refusal> {
    let needs_end = req.action != MaintenanceAction::End;
    if needs_end && req.for_secs.is_none_or(|s| s <= 0) {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "maintenance needs an end in the future",
        ));
    }
    let mut body = json!({ "action": req.action.as_str(), "reason": req.reason });
    if needs_end {
        body["for_secs"] = json!(req.for_secs);
    }
    let answer =
        passed_on(call(reqwest::Method::POST, "/v1/maintenance", actor, Some(body)).await)?;
    let detail = match req.for_secs {
        Some(s) if needs_end => format!("for {s}s: {}", req.reason),
        _ => req.reason.clone(),
    };
    let action = format!("ops.maintenance.{}", req.action.as_str());
    crate::handlers::audit::record(actor, &action, "maintenance", &detail);
    Ok(answer)
}
