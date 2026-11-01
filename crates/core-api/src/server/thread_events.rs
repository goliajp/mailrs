//! A route layer that turns a successful thread call into a thread event.
//!
//! Both cores mount it on their business routes, so a change made through
//! either core — from the web, a batch, MCP or JMAP, all of which reach the
//! core through these calls — raises the same event. What is raised is
//! decided once, in [`crate::webhook_events::thread_event`]; where it is
//! queued is the core's, through the sink it passes.

use std::collections::HashMap;
use std::sync::Arc;

use axum::RequestExt as _;
use axum::extract::{MatchedPath, Path, Request, State};
use axum::middleware::Next;
use axum::response::Response;

use crate::webhook_events::{ThreadChange, thread_event};

/// One thread event, as the layer saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadEventHit {
    pub user: String,
    pub thread_id: String,
    /// The label, for `thread.labelled`.
    pub label_id: Option<String>,
    pub change: ThreadChange,
}

/// Where a core queues what the layer saw. Called after the response is
/// ready, for 2xx answers only.
pub type ThreadEventSink = Arc<dyn Fn(ThreadEventHit) + Send + Sync>;

/// The layer: pass the request on, and on success hand the event to `sink`.
pub async fn thread_events(
    State(sink): State<ThreadEventSink>,
    mut req: Request,
    next: Next,
) -> Response {
    let change = req
        .extensions()
        .get::<MatchedPath>()
        .and_then(|p| thread_event(p.as_str(), req.method().as_str()));
    let Some(change) = change else {
        return next.run(req).await;
    };
    let params = req
        .extract_parts::<Path<HashMap<String, String>>>()
        .await
        .map(|Path(p)| p)
        .unwrap_or_default();
    let res = next.run(req).await;
    if res.status().is_success()
        && let (Some(user), Some(thread_id)) = (params.get("user"), params.get("thread_id"))
    {
        sink(ThreadEventHit {
            user: user.clone(),
            thread_id: thread_id.clone(),
            label_id: params.get("label_id").cloned(),
            change,
        });
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::method::label::PATH_THREAD_LABEL;
    use crate::method::thread::{PATH_ARCHIVE, PATH_LIST_THREAD_MESSAGES};
    use axum::Router;
    use axum::body::Body;
    use axum::http::StatusCode;
    use axum::routing::post;
    use std::sync::Mutex;
    use tower::ServiceExt as _;

    fn app(seen: Arc<Mutex<Vec<ThreadEventHit>>>, status: StatusCode) -> Router {
        let sink: ThreadEventSink = Arc::new(move |hit| seen.lock().unwrap().push(hit));
        Router::new()
            .route(PATH_ARCHIVE, post(move || async move { status }))
            .route(
                PATH_THREAD_LABEL,
                post(|| async { StatusCode::NO_CONTENT })
                    .delete(|| async { StatusCode::NO_CONTENT }),
            )
            .route(PATH_LIST_THREAD_MESSAGES, post(|| async { StatusCode::OK }))
            .route_layer(axum::middleware::from_fn_with_state(sink, thread_events))
    }

    async fn call(app: Router, method: &str, uri: &str) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        app.oneshot(req).await.unwrap();
    }

    #[tokio::test]
    async fn a_successful_thread_call_is_seen_with_its_decoded_ids() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        call(
            app(seen.clone(), StatusCode::OK),
            "POST",
            "/v1/users/a%40x.test/threads/t%3A1/archive",
        )
        .await;
        call(
            app(seen.clone(), StatusCode::OK),
            "DELETE",
            "/v1/users/a%40x.test/threads/t2/labels/l7",
        )
        .await;
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].user, "a@x.test");
        assert_eq!(seen[0].thread_id, "t:1");
        assert_eq!(seen[0].change.field, "archived");
        assert_eq!(seen[1].label_id.as_deref(), Some("l7"));
        assert!(!seen[1].change.on);
    }

    #[tokio::test]
    async fn a_failed_call_and_other_calls_are_not_events() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        call(
            app(seen.clone(), StatusCode::NOT_FOUND),
            "POST",
            "/v1/users/a/threads/t/archive",
        )
        .await;
        call(
            app(seen.clone(), StatusCode::OK),
            "POST",
            "/v1/users/a/threads/t/messages",
        )
        .await;
        assert!(seen.lock().unwrap().is_empty());
    }
}
