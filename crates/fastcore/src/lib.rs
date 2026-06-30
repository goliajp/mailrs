//! `mailrs-fastcore` — Kevy-backed implementation of the
//! `mailrs-core-api` server surface. Phase 8.
//!
//! Today this binary mounts a small subset:
//! - `/v1/healthz` + `/v1/readyz` (open) — proves the role works
//! - `POST /v1/users/{user}/conversations:list` — Rock 1 read path
//!
//! The rest of the 87-route surface fills in as `mailbox-kevy` grows
//! method coverage. Run alongside (or instead of) the monolith core
//! to A/B test conversation-list latency under the same load.
//!
//! Environment:
//! - `MAILRS_FASTCORE_BIND` — listen address (default `0.0.0.0:3301`,
//!   one above the monolith's core-rpc :3300 so both can coexist)
//! - `MAILRS_KEVY_DATA_DIR` — kevy persist dir (default
//!   `/data/kevy-fastcore`)

#![allow(missing_docs)]

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use kevy_embedded::{Config, Store};
use mailrs_core_api::method::conversation as conv;
use mailrs_core_api::method::thread as th;
use mailrs_core_api::server::{Handler, base_router};
use mailrs_core_api::types::{BackendKind, ConversationSummaryWire, HealthResponse};
use mailrs_mailbox_kevy::{KevyMailboxStore, ListThreadsFilter, ThreadRow};

/// Server state — owns the kevy store and is cloned into axum handlers.
pub struct FastcoreState {
    pub mailbox: KevyMailboxStore,
}

impl Handler for FastcoreState {
    async fn healthz(&self) -> HealthResponse {
        HealthResponse {
            version: mailrs_core_api::API_VERSION.into(),
            backend: BackendKind::Kevy,
            ready: true,
        }
    }

    async fn readyz(&self) -> HealthResponse {
        // kevy is in-process; if the binary is up, the store is up.
        HealthResponse {
            version: mailrs_core_api::API_VERSION.into(),
            backend: BackendKind::Kevy,
            ready: true,
        }
    }
}

pub async fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let kevy_dir =
        std::env::var("MAILRS_KEVY_DATA_DIR").unwrap_or_else(|_| "/data/kevy-fastcore".to_string());
    let cfg = Config::default().with_persist(&kevy_dir);
    let store = Arc::new(Store::open(cfg).expect("open kevy store"));
    let mailbox = KevyMailboxStore::new(store);
    let state = Arc::new(FastcoreState { mailbox });

    let addr = std::env::var("MAILRS_FASTCORE_BIND").unwrap_or_else(|_| "0.0.0.0:3301".into());

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("bind MAILRS_FASTCORE_BIND");
    tracing::info!(
        addr = %addr,
        kevy_dir = %kevy_dir,
        "mailrs-fastcore listening (kevy backend)"
    );
    axum::serve(listener, app).await.unwrap();
}

fn build_router(state: Arc<FastcoreState>) -> Router {
    let base = base_router(state.clone());
    let convo = Router::new()
        .route(conv::PATH_LIST_CONVERSATIONS, post(list_conversations))
        .route(conv::PATH_CONVERSATION_CATEGORIES, get(get_categories))
        .route(conv::PATH_ACTION_COUNT, get(get_action_count))
        .route(conv::PATH_UNSEEN_COUNT, get(get_unseen_count));

    let thread = Router::new()
        .route(th::PATH_MARK_READ, post(mark_read))
        .route(th::PATH_PIN, post(pin_thread))
        .route(th::PATH_UNPIN, post(unpin_thread))
        .route(th::PATH_ARCHIVE, post(archive_thread))
        .route(th::PATH_UNARCHIVE, post(unarchive_thread))
        .route(th::PATH_DISMISS_ACTION, post(dismiss_action))
        .route(th::PATH_DELETE_THREAD, delete(delete_thread));

    base.merge(convo.merge(thread).with_state(state))
}

fn row_to_wire(r: ThreadRow) -> ConversationSummaryWire {
    ConversationSummaryWire {
        thread_id: r.thread_id,
        subject: r.subject,
        participants: r.senders_csv,
        message_count: r.count.max(0) as u32,
        unread_count: r.unread_count.max(0) as u32,
        last_date: r.latest_date,
        category: r.category,
        flagged: r.requires_action,
        snippet: r.latest_preview,
        pinned: r.pinned,
        archived: r.archived,
        importance_level: r.importance_level,
        importance_score: r.importance_score as f32,
        requires_action: r.requires_action,
        last_sender: String::new(), // not yet tracked on the kevy row
        sent_count: r.sent_count.max(0) as u32,
    }
}

/// `POST /v1/users/{user}/conversations:list`.
async fn list_conversations(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<conv::ListConversationsRequest>,
) -> Json<conv::ListConversationsResponse> {
    let f = &req.filter;
    let filter = ListThreadsFilter {
        category: f.category.as_deref(),
        pinned: false,
        archived: f.archived,
        has_unread: f.unread.unwrap_or(false),
        has_action: false,
    };
    let limit = if f.limit == 0 { 50 } else { f.limit as usize };
    let (rows, _total) = state
        .mailbox
        .list_threads_by_activity(&user, &filter, 0, limit)
        .unwrap_or_else(|_| (Vec::new(), 0));

    let items = rows.into_iter().map(row_to_wire).collect();
    Json(conv::ListConversationsResponse { items })
}

/// `GET /v1/users/{user}/conversations/categories` — histogram of
/// category → distinct thread_id count, read straight off the per-
/// category zsets.
async fn get_categories(
    State(state): State<Arc<FastcoreState>>,
    Path(_user): Path<String>,
) -> Json<conv::ConversationCategoriesResponse> {
    // Hardcoded set for now — mailbox-kevy doesn't have a "list all
    // categories for user" iterator yet. The 5 below match the UI tab
    // set; missing ones return 0.
    let categories: Vec<conv::CategoryCount> = ["personal", "bulk", "spam", "scam", "inbox"]
        .into_iter()
        .map(|cat| {
            let key = mailrs_mailbox_kevy::keys::user_threads_by_category(&_user, cat);
            let count = state.mailbox.store_ref().zcard(key.as_bytes()).unwrap_or(0) as i64;
            conv::CategoryCount {
                category: cat.to_string(),
                count,
            }
        })
        .filter(|c| c.count > 0)
        .collect();
    Json(conv::ConversationCategoriesResponse { categories })
}

/// `GET /v1/users/{user}/conversations/action-count` — single ZCARD on
/// the has_action zset.
async fn get_action_count(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Json<conv::ActionCountResponse> {
    let key = mailrs_mailbox_kevy::keys::user_threads_has_action(&user);
    let count = state.mailbox.store_ref().zcard(key.as_bytes()).unwrap_or(0) as i64;
    Json(conv::ActionCountResponse { count })
}

/// `GET /v1/users/{user}/conversations/unseen-count` — single ZCARD on
/// the has_unread zset.
async fn get_unseen_count(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Json<conv::UnseenCountResponse> {
    let key = mailrs_mailbox_kevy::keys::user_threads_has_unread(&user);
    let count = state.mailbox.store_ref().zcard(key.as_bytes()).unwrap_or(0) as i64;
    Json(conv::UnseenCountResponse { count })
}

// ── Thread mutations ───────────────────────────────────────────────

/// 204 if mailbox-kevy reports the row existed + was mutated; 404 if
/// the thread row is missing. Most mutations are idempotent so a
/// 2nd call lands the same status.
fn status_for(found: bool) -> StatusCode {
    if found {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn mark_read(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> StatusCode {
    status_for(state.mailbox.mark_seen(&user, &thread_id).unwrap_or(false))
}

async fn pin_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> StatusCode {
    status_for(
        state
            .mailbox
            .set_pinned(&user, &thread_id, true)
            .unwrap_or(false),
    )
}

async fn unpin_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> StatusCode {
    status_for(
        state
            .mailbox
            .set_pinned(&user, &thread_id, false)
            .unwrap_or(false),
    )
}

async fn archive_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> StatusCode {
    status_for(
        state
            .mailbox
            .set_archived(&user, &thread_id, true)
            .unwrap_or(false),
    )
}

async fn unarchive_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> StatusCode {
    status_for(
        state
            .mailbox
            .set_archived(&user, &thread_id, false)
            .unwrap_or(false),
    )
}

async fn dismiss_action(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> StatusCode {
    status_for(
        state
            .mailbox
            .set_has_action(&user, &thread_id, false)
            .unwrap_or(false),
    )
}

async fn delete_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> StatusCode {
    status_for(
        state
            .mailbox
            .delete_thread(&user, &thread_id)
            .unwrap_or(false),
    )
}
