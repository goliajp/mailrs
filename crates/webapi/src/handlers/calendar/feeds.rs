//! `/api/calendar/feeds` — subscriptions to external ICS URLs.

use axum::Json;
use axum::extract::{Extension, Path};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The stored row, shared with the worker that syncs it.
///
/// Was a local type holding only what the form collected; the sync state the
/// worker records (`last_synced_at`, `last_error`, the validators) now comes
/// back with the feed, so a subscription that has been failing says so on
/// the page where it was added.
pub type FeedWire = mailrs_core_sidestate::families::calendar_feeds::FeedRow;

/// A feed as the client sees it.
///
/// Separate from the stored row because the row holds the basic-auth
/// password the user typed. Serving the stored row directly would send that
/// password back to the browser on every list — and `FeedRow` has to keep it
/// serializable, since that is how the worker reads it.
#[derive(Debug, Serialize)]
pub struct FeedView {
    pub id: String,
    pub name: String,
    pub url: String,
    pub color: Option<String>,
    pub sync_interval_secs: i64,
    pub created_at: i64,
    pub last_synced_at: i64,
    pub last_error: Option<String>,
    pub last_event_count: i64,
    /// Whether credentials are stored, without saying what they are.
    pub has_basic_auth: bool,
}

impl From<FeedWire> for FeedView {
    fn from(f: FeedWire) -> Self {
        Self {
            id: f.id,
            name: f.name,
            url: f.url,
            color: f.color,
            sync_interval_secs: f.sync_interval_secs,
            created_at: f.created_at,
            last_synced_at: f.last_synced_at,
            last_error: f.last_error,
            last_event_count: f.last_event_count,
            has_basic_auth: f.basic_auth_user.is_some(),
        }
    }
}

#[derive(Debug, Deserialize)]
/// `deny_unknown_fields`: the basic-auth pair was dropped here once
/// already, silently, because the handler did not name it. Pinned by
/// `wire-contract/requests/calendar-feed-create.json`.
#[serde(deny_unknown_fields)]
pub struct CreateFeedRequest {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default = "default_sync_interval")]
    pub sync_interval_secs: i64,
    /// Username for a feed behind HTTP basic auth.
    ///
    /// The inputs were removed on 2026-07-30 because nothing consumed them.
    /// The fetcher that does now exists.
    #[serde(default)]
    pub basic_auth_user: Option<String>,
    /// Password for a feed behind HTTP basic auth.
    #[serde(default)]
    pub basic_auth_pass: Option<String>,
}

fn default_sync_interval() -> i64 {
    3600
}

fn random_id() -> String {
    let mut b = [0u8; 8];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// GET /api/calendar/feeds — user's subscribed feeds.
pub async fn list_feeds(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Json<serde_json::Value> {
    let key = mailrs_core_sidestate::families::calendar_feeds::feeds_key(&user);
    let flat = with_kevy(move |c| c.hgetall(key.as_bytes()).map_err(std::io::Error::from))
        .unwrap_or_default();
    let mut items: Vec<FeedView> = Vec::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        if let Ok(f) = serde_json::from_slice::<FeedWire>(&flat[i + 1]) {
            items.push(f.into());
        }
        i += 2;
    }
    Json(serde_json::json!({ "items": items }))
}

/// POST /api/calendar/feeds — subscribe.
pub async fn create_feed(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<CreateFeedRequest>,
) -> Result<Json<FeedView>, StatusCode> {
    let feed = FeedWire {
        id: random_id(),
        name: req.name,
        url: req.url,
        color: req.color,
        sync_interval_secs: req.sync_interval_secs,
        created_at: now_secs(),
        // Never synced, so the worker picks it up on its next tick rather
        // than an interval from now.
        last_synced_at: 0,
        last_error: None,
        last_event_count: 0,
        etag: None,
        last_modified: None,
        basic_auth_user: req.basic_auth_user,
        basic_auth_pass: req.basic_auth_pass,
    };
    let key = mailrs_core_sidestate::families::calendar_feeds::feeds_key(&user);
    let id_c = feed.id.clone();
    let payload = serde_json::to_vec(&feed).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(id_c.as_bytes(), payload.as_slice())])?;
        Ok(())
    })?;
    Ok(Json(feed.into()))
}

/// DELETE /api/calendar/feeds/{feed_id} — unsubscribe.
pub async fn delete_feed(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(feed_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let key = mailrs_core_sidestate::families::calendar_feeds::feeds_key(&user);
    with_kevy(move |c| {
        c.hdel(key.as_bytes(), &[feed_id.as_bytes()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}
