//! `GET /api/admin/auth-events`: the login facts every way in records, for
//! an administrator asking who tried an account, from where, and how.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use mailrs_auth_guard::events::{self, LoginEvent};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

/// GET /api/admin/auth-events?account=&source=&result=&channel=&since=&until=&limit=
///
/// Newest first, at most `limit` (100 by default, 1000 at most); `since`
/// and `until` are unix seconds. Super administrators only.
pub async fn list_auth_events(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Query(q): Query<events::Query>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let items = auth_events(&state, &actor, q).await?;
    Ok(Json(serde_json::json!({ "items": items })))
}

/// Shared by the REST route and the MCP tool.
pub(crate) async fn auth_events(
    state: &Arc<WebState>,
    actor: &str,
    q: events::Query,
) -> Result<Vec<LoginEvent>, StatusCode> {
    crate::handlers::kevy_util::require_super(state, actor).await?;
    crate::login_guard::GUARD.events(q).await.map_err(|e| {
        tracing::warn!(error = %e, "auth events: store unreachable");
        StatusCode::SERVICE_UNAVAILABLE
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Uri;

    #[test]
    fn the_query_string_reads_into_a_query() {
        let uri: Uri = "/x?account=a%40x.test&source=192.0.2.1&result=bad-password&channel=imap&since=10&limit=5"
            .parse()
            .unwrap();
        let Query(q) = Query::<events::Query>::try_from_uri(&uri).unwrap();
        assert_eq!(q.account.as_deref(), Some("a@x.test"));
        assert_eq!(q.result, Some(events::Outcome::BadPassword));
        assert_eq!(q.channel, Some(events::Channel::Imap));
        assert_eq!((q.since, q.limit), (Some(10), Some(5)));
        assert!(Query::<events::Query>::try_from_uri(&"/x?result=nope".parse().unwrap()).is_err());
    }
}
