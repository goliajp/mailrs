//! JMAP (RFC 8620 core, RFC 8621 mail): the session document, the API
//! endpoint, uploads and downloads, and push.
//!
//! Ids: a message is `M{uid}` (the per-user uid both cores resolve, which a
//! move does not change), a thread `T` + a hash of its id, a mailbox a role
//! name or `L` + a label id. State: see `state`.

pub mod auth;
mod blob;
mod changes;
mod ctx;
mod email;
mod email_query;
mod email_set;
mod identity;
mod ids;
mod import;
mod limits;
mod mailbox;
mod mailbox_set;
pub(crate) mod masked;
mod push;
mod session;
mod state;
mod submission;
mod thread;
mod view;

use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Extension, State};
use axum::response::{IntoResponse, Response};
use mailrs_jmap::dispatch::{JMAP_CORE_CAP, JMAP_MAIL_CAP, JMAP_SUBMISSION_CAP, JmapRequest};
use mailrs_jmap::error::JmapMethodError;
use serde_json::{Value, json};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

pub use blob::{download, health as upload_health, spawn_sweep as spawn_upload_sweep, upload};
pub use push::eventsource;
pub use session::session;

use ctx::Ctx;
pub(crate) use limits::{MAX_OBJECTS, MAX_SIZE_REQUEST, MAX_SIZE_UPLOAD, limit_error};

/// The capabilities this server has; a request naming any other is refused.
const KNOWN_CAPS: [&str; 4] = [
    JMAP_CORE_CAP,
    JMAP_MAIL_CAP,
    JMAP_SUBMISSION_CAP,
    masked::MASKED_CAP,
];

/// The view and state the calls of one request read, made on first need
/// and again after a call that wrote.
struct Now {
    view: Arc<view::View>,
    mailboxes: Vec<(String, Value)>,
    state: state::Now,
}

async fn now(ctx: &Ctx) -> Result<Now, JmapMethodError> {
    let view = view::current(ctx).await?;
    let mailboxes = mailbox::objects(ctx, &view).await?;
    let state = state::now(&ctx.user, &view, &mailboxes)?;
    Ok(Now {
        view,
        mailboxes,
        state,
    })
}

/// The capability a method needs in `using`.
fn capability_of(method: &str) -> &'static str {
    match method.split('/').next().unwrap_or("") {
        "Core" => JMAP_CORE_CAP,
        "EmailSubmission" | "Identity" => JMAP_SUBMISSION_CAP,
        "MaskedEmail" => masked::MASKED_CAP,
        _ => JMAP_MAIL_CAP,
    }
}

const WRITES: [&str; 4] = [
    "Mailbox/set",
    "Email/set",
    "Email/import",
    "EmailSubmission/set",
];

/// Run one method call. Most answer one response; `EmailSubmission/set`
/// with `onSuccessUpdateEmail` answers two.
async fn call(
    ctx: &Ctx,
    n: &Now,
    method: &str,
    args: &Value,
) -> Result<Vec<(String, Value)>, JmapMethodError> {
    let state = n.state.state.as_str();
    if WRITES.contains(&method)
        && let Some(want) = args.get("ifInState").and_then(|v| v.as_str())
        && want != state
    {
        return Err(JmapMethodError::Other {
            kind: "stateMismatch".into(),
            description: String::new(),
        });
    }
    let v = &n.view;
    let one = match method {
        "Core/echo" => Ok(("Core/echo".to_string(), args.clone())),
        "Mailbox/get" => mailbox::get(ctx, &n.mailboxes, args, state),
        "Mailbox/query" => mailbox::query(ctx, args, state).await,
        "Mailbox/changes" => changes::mailbox(ctx, &n.state, args),
        "Mailbox/queryChanges" => changes::cannot(),
        "Mailbox/set" => mailbox_set::set(ctx, args, state).await,
        "Email/get" => email::get(ctx, v, args, state).await,
        "Email/query" => email_query::query(ctx, v, args, state).await,
        "Email/changes" => changes::email(ctx, &n.state, args),
        "Email/queryChanges" => changes::email_query(ctx, v, &n.state, args).await,
        "Email/set" => email_set::set(ctx, v, args, state).await,
        "Email/import" => import::import(ctx, args, state).await,
        "Thread/get" => thread::get(ctx, v, args, state),
        "Thread/changes" => changes::thread(ctx, &n.state, args),
        "Identity/get" => identity::get(ctx, args).await,
        "EmailSubmission/set" => return submission::set(ctx, v, args, state).await,
        other => Err(JmapMethodError::UnknownMethod(other.to_string())),
    };
    one.map(|r| vec![r])
}

/// A request-level error (RFC 8620 §3.6.1): HTTP 400, problem details.
fn problem(kind: &str, detail: &str, limit: Option<&str>) -> Response {
    let mut body = json!({ "type": kind, "status": 400, "detail": detail });
    if let Some(l) = limit {
        body["limit"] = json!(l);
    }
    (
        axum::http::StatusCode::BAD_REQUEST,
        [(axum::http::header::CONTENT_TYPE, "application/problem+json")],
        body.to_string(),
    )
        .into_response()
}

/// POST /jmap — the API endpoint.
pub async fn api(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    scopes: Option<Extension<crate::session::AgentScopes>>,
    key: Option<Extension<crate::session::AgentKey>>,
    body: Bytes,
) -> Response {
    if body.len() > limits::MAX_SIZE_REQUEST {
        return problem(limits::LIMIT, "request too large", Some("maxSizeRequest"));
    }
    let Some(_slot) = limits::RequestSlot::take(&user) else {
        return problem(
            limits::LIMIT,
            "too many concurrent requests",
            Some("maxConcurrentRequests"),
        );
    };
    let req: JmapRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return problem(
                "urn:ietf:params:jmap:error:notRequest",
                &e.to_string(),
                None,
            );
        }
    };
    // An IETF capability this server lacks is let through: client libraries
    // name every one they know in each request (jmap-client sends
    // vacationresponse with a Mailbox/get), and a method that needs one is
    // still answered unknownMethod.
    let unknown =
        |c: &&String| !KNOWN_CAPS.contains(&c.as_str()) && !c.starts_with("urn:ietf:params:jmap:");
    if let Some(cap) = req.using.iter().find(unknown) {
        return problem(
            "urn:ietf:params:jmap:error:unknownCapability",
            &format!("unknown capability {cap}"),
            None,
        );
    }
    if req.method_calls.len() > limits::MAX_CALLS {
        return problem(
            limits::LIMIT,
            "too many method calls",
            Some("maxCallsInRequest"),
        );
    }
    let masked_only = scopes
        .as_ref()
        .is_some_and(|Extension(s)| crate::session::masked_only(&s.0));
    let created_by = match &key {
        Some(Extension(k)) => format!("key {}", k.prefix),
        None => "jmap".to_string(),
    };
    let ctx = Ctx::new(state.clone(), user.clone(), key.map(|Extension(k)| k));
    let mut current: Option<Now> = None;
    let mut responses: Vec<(String, Value, String)> = Vec::new();
    for (method, mut args, call_id) in req.method_calls {
        mailrs_jmap::refs::resolve_references(&mut args, &responses);
        let answered = match masked::dispatch(&state, &user, &created_by, &method, &args).await {
            Some(done) => done.map(|r| vec![r]),
            None if masked_only => Err(JmapMethodError::Other {
                kind: "forbidden".into(),
                description: "this key may only manage masked addresses".into(),
            }),
            None if !req.using.iter().any(|c| c == capability_of(&method)) => Err(
                JmapMethodError::UnknownMethod(format!("{method}: its capability is not in using")),
            ),
            None => match current.take() {
                Some(n) => run(&ctx, n, &method, &args, &mut current).await,
                None => match now(&ctx).await {
                    Ok(n) => run(&ctx, n, &method, &args, &mut current).await,
                    Err(e) => Err(e),
                },
            },
        };
        match answered {
            Ok(list) => responses.extend(
                list.into_iter()
                    .map(|(name, value)| (name, value, call_id.clone())),
            ),
            Err(e) => responses.push(("error".to_string(), e.to_json(), call_id)),
        }
    }
    Json(mailrs_jmap::dispatch::JmapResponse {
        method_responses: responses,
        session_state: session::state_of(&user),
    })
    .into_response()
}

/// Run a call against `n`, and leave in `current` the view the next call
/// reads: `n` again, or a fresh one after a write, whose state becomes
/// the write's `newState`.
async fn run(
    ctx: &Ctx,
    n: Now,
    method: &str,
    args: &Value,
    current: &mut Option<Now>,
) -> Result<Vec<(String, Value)>, JmapMethodError> {
    let result = call(ctx, &n, method, args).await;
    if !WRITES.contains(&method) {
        *current = Some(n);
        return result;
    }
    let after = now(ctx).await?;
    let mut result = result?;
    for (name, value) in result.iter_mut() {
        if name.ends_with("/set") || name == "Email/import" {
            value["newState"] = json!(after.state.state);
        }
    }
    *current = Some(after);
    Ok(result)
}
