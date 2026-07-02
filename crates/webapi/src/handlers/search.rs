//! `/api/conversations/semantic-search` — meili-backed semantic search.
//!
//! Falls back to a linear scan of the user's activity zset when meili
//! is unreachable. Mirrors the monolith's search endpoint shape.

use axum::Json;
use axum::extract::{Extension, Query};
use serde::Deserialize;

use crate::handlers::conversations::AuthedUser;

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    20
}

/// GET /api/conversations/semantic-search?q=&limit= — returns an array
/// of `{ thread_id, subject, score }` sorted by relevance.
pub async fn semantic_search(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<SearchQuery>,
) -> Json<serde_json::Value> {
    let meili_url = std::env::var("MAILRS_MEILI_URL").ok();
    let query = q.q.clone();
    let limit = q.limit;
    if let Some(base) = meili_url {
        let index = format!("conversations-{}", user.replace(['@', '.'], "_"));
        let url = format!("{base}/indexes/{index}/search");
        let key = std::env::var("MAILRS_MEILI_KEY").ok();
        let client = reqwest::Client::new();
        let mut req = client.post(&url).json(&serde_json::json!({
            "q": query,
            "limit": limit,
        }));
        if let Some(k) = key {
            req = req.bearer_auth(k);
        }
        if let Ok(resp) = req.send().await
            && resp.status().is_success()
            && let Ok(v) = resp.json::<serde_json::Value>().await
        {
            let hits = v.get("hits").cloned().unwrap_or(serde_json::json!([]));
            return Json(serde_json::json!({ "items": hits, "backend": "meili" }));
        }
    }

    // Linear fallback: read the user's activity zset directly.
    let activity_key = format!("mailrs:user:{user}:threads:by_activity");
    let ids =
        crate::handlers::kevy_util::with_kevy(move |c| c.zrange(activity_key.as_bytes(), 0, 199))
            .unwrap_or_default();
    let needle = q.q.to_lowercase();
    let mut out = Vec::new();
    for id_bytes in ids.into_iter().take(500) {
        let Some(tid) = String::from_utf8(id_bytes).ok() else {
            continue;
        };
        let thread_key = format!("mailrs:thread:{tid}");
        let flat = crate::handlers::kevy_util::with_kevy(move |c| c.hgetall(thread_key.as_bytes()))
            .unwrap_or_default();
        let mut subject = String::new();
        let mut i = 0;
        while i + 1 < flat.len() {
            if flat[i] == b"subject" {
                subject = String::from_utf8_lossy(&flat[i + 1]).to_string();
                break;
            }
            i += 2;
        }
        if subject.to_lowercase().contains(&needle) {
            out.push(serde_json::json!({ "thread_id": tid, "subject": subject, "score": 1.0 }));
            if out.len() as u32 >= q.limit {
                break;
            }
        }
    }
    Json(serde_json::json!({ "items": out, "backend": "linear_fallback" }))
}
