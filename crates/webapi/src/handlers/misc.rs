//! Miscellaneous fastcore-native handlers — keys, deliverability
//! check, spam-feedback, mbox export, search.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Query, State},
    http::StatusCode,
    response::IntoResponse,
};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

fn with_kevy<F, T>(f: F) -> Result<T, StatusCode>
where
    F: FnOnce(&mut kevy_client::Connection) -> std::io::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let url = std::env::var("MAILRS_KEVY_URL").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let handle = std::thread::spawn(move || -> std::io::Result<T> {
        let mut c = kevy_client::Connection::open(&url)?;
        f(&mut c)
    });
    handle
        .join()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn map_core_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

// ── PGP keys (network kevy hash) ──────────────────────────────────
//
// Keys:
//   pgp_keys:<user>       hash  { <email> -> <ascii-armored public key> }

/// GET /api/mail/keys — list saved public keys for the current user.
pub async fn get_keys(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("pgp_keys:{user}");
    let flat = with_kevy(move |c| c.hgetall(key.as_bytes()))?;
    let mut items = Vec::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        items.push(serde_json::json!({
            "email": String::from_utf8_lossy(&flat[i]),
            "key_armored": String::from_utf8_lossy(&flat[i + 1]),
        }));
        i += 2;
    }
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, serde::Deserialize)]
pub struct SaveKeyRequest {
    pub email: String,
    pub key_armored: String,
}

/// POST /api/mail/keys — upsert a key entry.
pub async fn save_key(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<SaveKeyRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("pgp_keys:{user}");
    let email = req.email;
    let body = req.key_armored;
    with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(email.as_bytes(), body.as_bytes())])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Deliverability check ─────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct DeliverabilityQuery {
    pub domain: String,
}

/// GET /api/mail/check-deliverability?domain=example.com —
/// look up SPF, DKIM (default._domainkey), and DMARC TXT records for
/// the target domain and return a summary. External DNS only.
pub async fn check_deliverability(
    Query(q): Query<DeliverabilityQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use hickory_resolver::config::{ResolverConfig, ResolverOpts};
    let resolver = hickory_resolver::TokioAsyncResolver::tokio(
        ResolverConfig::default(),
        ResolverOpts::default(),
    );

    async fn txt_join(
        resolver: &hickory_resolver::TokioAsyncResolver,
        name: &str,
    ) -> Option<String> {
        let l = resolver.txt_lookup(name).await.ok()?;
        let joined: Vec<String> = l
            .iter()
            .map(|txt| {
                txt.txt_data()
                    .iter()
                    .flat_map(|b| std::str::from_utf8(b).ok().map(str::to_owned))
                    .collect::<String>()
            })
            .collect();
        if joined.is_empty() {
            None
        } else {
            Some(joined.join("\n"))
        }
    }

    let spf = txt_join(&resolver, &q.domain).await;
    let dkim = txt_join(&resolver, &format!("default._domainkey.{}", q.domain)).await;
    let dmarc = txt_join(&resolver, &format!("_dmarc.{}", q.domain)).await;

    Ok(Json(serde_json::json!({
        "domain": q.domain,
        "spf": spf,
        "dkim": dkim,
        "dmarc": dmarc,
    })))
}

// ── Spam feedback (network kevy hash) ─────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct SpamFeedbackRequest {
    pub message_id: String,
    pub is_spam: bool,
}

/// POST /api/mail/spam-feedback — record the user's spam vote.
/// Persisted to `spam_feedback:<user>` hash for eventual bulk export
/// to whatever trainer we wire up later.
pub async fn spam_feedback(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<SpamFeedbackRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("spam_feedback:{user}");
    let mid = req.message_id;
    let val = if req.is_spam { "spam" } else { "ham" };
    with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(mid.as_bytes(), val.as_bytes())])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Mbox export ──────────────────────────────────────────────────

/// GET /api/mail/export — stream every message in the user's inbox as
/// a single mbox file. Walks the activity zset and concatenates each
/// message's maildir bytes prefixed with `From ...` per RFC 4155.
pub async fn export_mbox(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<axum::response::Response, StatusCode> {
    use mailrs_message_store::MessageStore;
    let maildir_root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let Some((local, domain)) = user.split_once('@') else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let path = format!("{maildir_root}/{domain}/{local}");
    let store = mailrs_message_store::MaildirStore;

    // Pull the full list of thread_ids via fastcore (single conversation
    // list call with a large limit).
    let req = mailrs_core_api::method::conversation::ListConversationsRequest {
        filter: mailrs_core_api::types::ConversationFilter {
            limit: 10_000,
            before_ts: None,
            category: None,
            domains: None,
            archived: false,
            folder: None,
            unread: None,
            starred: None,
            section: None,
        },
    };
    let convs = state
        .core
        .list_conversations(&user, &req)
        .await
        .map_err(map_core_err)?;

    let mut out = Vec::<u8>::new();
    for c in convs.items {
        // Every message in each thread — fetch via list_thread_messages RPC.
        if let Ok(resp) = state.core.list_thread_messages(&user, &c.thread_id).await {
            for w in resp.items {
                if let Ok(Some(bytes)) = store
                    .fetch(&path, &mailrs_message_store::MessageId(w.blob_ref.clone()))
                    .await
                {
                    out.extend_from_slice(b"From MAILER-DAEMON ");
                    let _ = writeln_epoch(&mut out, w.internal_date);
                    out.extend_from_slice(&bytes);
                    if !bytes.ends_with(b"\n") {
                        out.push(b'\n');
                    }
                }
            }
        }
    }
    Ok((
        [
            ("content-type", "application/mbox"),
            ("content-disposition", "attachment; filename=inbox.mbox"),
        ],
        out,
    )
        .into_response())
}

fn writeln_epoch(out: &mut Vec<u8>, epoch: i64) -> std::io::Result<()> {
    use std::io::Write;
    // POSIX asctime shape for the mbox From-line: `YYYY Mon  D HH:MM:SS`.
    // Prior version hard-coded 1970-01-01 so every exported message
    // sorted identically in downstream mbox clients. Compute the real
    // Gregorian date from the epoch.
    static MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let secs = epoch.max(0) as u64;
    let mut days = secs / 86_400;
    let sec_of_day = secs % 86_400;
    let hour = (sec_of_day / 3600) as u32;
    let minute = ((sec_of_day % 3600) / 60) as u32;
    let second = (sec_of_day % 60) as u32;
    let mut year: u32 = 1970;
    loop {
        let leap =
            (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
        let ydays: u64 = if leap { 366 } else { 365 };
        if days < ydays {
            break;
        }
        days -= ydays;
        year += 1;
    }
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let ml = [
        31u64,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0usize;
    while month < 12 && days >= ml[month] {
        days -= ml[month];
        month += 1;
    }
    let day = days + 1;
    writeln!(
        out,
        "{year} {mon} {day:>2} {hour:02}:{minute:02}:{second:02}",
        mon = MONTHS[month.min(11)]
    )?;
    Ok(())
}

// ── Search ────────────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_search_limit")]
    pub limit: u32,
}

fn default_search_limit() -> u32 {
    50
}

/// GET /api/conversations/search?q=&limit= — full-text search across
/// the user's threads.
///
/// Ranked by the kevy text index that fastcore maintains inside the
/// same store as the rows, then hydrated through the by-thread-ids RPC
/// so the UI gets unread / flags / category like a normal list. This
/// used to query a Meilisearch sidecar, which was removed: it was a
/// second copy of the index that could drift from the rows, and did —
/// silently, for weeks (2026-07-19).
pub async fn search_conversations(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Vec<crate::handlers::conversations::ConversationResponse>>, StatusCode> {
    let req = mailrs_core_api::method::conversation::SearchConversationsRequest {
        query: q.q,
        category: None,
        limit: q.limit,
    };
    let hits = state
        .core
        .search_conversations(&user, &req)
        .await
        .map_err(map_core_err)?;
    let thread_ids: Vec<String> = hits.items.iter().map(|c| c.thread_id.clone()).collect();
    if thread_ids.is_empty() {
        return Ok(Json(Vec::new()));
    }
    // hydrate full rows, preserving relevance order
    let hydrate = mailrs_core_api::method::conversation::ConversationsByIdsRequest {
        thread_ids,
        folder: None,
    };
    let rows = state
        .core
        .conversations_by_thread_ids(&user, &hydrate)
        .await
        .map_err(map_core_err)?;
    let out: Vec<crate::handlers::conversations::ConversationResponse> =
        rows.items.into_iter().map(Into::into).collect();
    Ok(Json(out))
}
