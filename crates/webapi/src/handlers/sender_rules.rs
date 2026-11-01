//! A reader's rules for one sender: correct a sender's conversations once
//! and have it remembered, undo that, and keep a sender's mail for a while.
//!
//! - `POST /api/senders/reclassify` moves the sender's conversations and
//!   writes the rule; the answer carries the correction to undo
//! - `POST /api/senders/corrections/{id}/undo` puts them back
//! - `GET /api/sender-rules`, `PUT|DELETE /api/sender-rules/{address}`

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use mailrs_core_api::method::senders as wire;
use mailrs_core_sidestate::families::sender_rules::{
    self as rules, Correction, Disposal, Moved, SenderRule,
};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

fn core_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

fn is_bucket(b: &str) -> bool {
    rules::BUCKETS.contains(&b)
}

#[derive(Debug, serde::Deserialize)]
pub struct ReclassifyBody {
    pub sender: String,
    pub bucket: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ReclassifyAnswer {
    /// Pass to the undo route; absent when nothing moved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correction_id: Option<String>,
    pub moved: usize,
}

/// POST /api/senders/reclassify
pub async fn reclassify(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(body): Json<ReclassifyBody>,
) -> Result<Json<ReclassifyAnswer>, StatusCode> {
    let sender = rules::normalize(&body.sender);
    if sender.is_empty() || !sender.contains('@') || !is_bucket(&body.bucket) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let req = wire::ReclassifySenderRequest {
        sender: sender.clone(),
        bucket: body.bucket.clone(),
    };
    let moved = state
        .core
        .reclassify_sender(&user, &req)
        .await
        .map_err(core_err)?
        .moved;
    let now = chrono::Utc::now().timestamp();
    let id = format!("{now}-{:08x}", rand_suffix());
    let fact = Correction::Moved {
        id: id.clone(),
        sender: sender.clone(),
        to_bucket: body.bucket.clone(),
        moved: moved
            .iter()
            .map(|m| Moved {
                thread_id: m.thread_id.clone(),
                from_bucket: m.from_bucket.clone(),
            })
            .collect(),
        at: now,
    };
    let (who, bucket, count) = (user.clone(), body.bucket, moved.len());
    with_kevy(move |c| {
        rules::record(c, &who, &fact)?;
        let rule = SenderRule {
            bucket: Some(bucket),
            source: "correction".into(),
            created_at: now,
            ..rules::get(c, &who, &sender)?.unwrap_or_default()
        };
        rules::put(c, &who, &sender, &rule)
    })?;
    let correction_id = (count > 0).then_some(id);
    Ok(Json(ReclassifyAnswer {
        correction_id,
        moved: count,
    }))
}

fn rand_suffix() -> u32 {
    use rand_core::RngCore;
    rand_core::OsRng.next_u32()
}

#[derive(Debug, serde::Serialize)]
pub struct UndoAnswer {
    pub restored: u32,
}

/// POST /api/senders/corrections/{id}/undo
pub async fn undo(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<String>,
) -> Result<Json<UndoAnswer>, StatusCode> {
    let (who, cid) = (user.clone(), id.clone());
    let open = with_kevy(move |c| rules::open_correction(c, &who, &cid))?;
    let Some(Correction::Moved {
        sender,
        to_bucket,
        moved,
        ..
    }) = open
    else {
        return Err(StatusCode::NOT_FOUND);
    };
    let req = wire::RestoreSenderRequest {
        sender: sender.clone(),
        to_bucket,
        moved: moved
            .into_iter()
            .map(|m| wire::MovedWire {
                thread_id: m.thread_id,
                from_bucket: m.from_bucket,
            })
            .collect(),
    };
    let restored = state
        .core
        .restore_sender(&user, &req)
        .await
        .map_err(core_err)?
        .restored;
    let now = chrono::Utc::now().timestamp();
    with_kevy(move |c| {
        rules::record(c, &user, &Correction::Undone { id, at: now })?;
        // the correction wrote the bucket; a retention set separately stays
        match rules::get(c, &user, &sender)? {
            Some(rule) if rule.retains() => rules::put(
                c,
                &user,
                &sender,
                &SenderRule {
                    bucket: None,
                    ..rule
                },
            ),
            Some(_) => rules::delete(c, &user, &sender).map(|_| ()),
            None => Ok(()),
        }
    })?;
    Ok(Json(UndoAnswer { restored }))
}

#[derive(Debug, serde::Serialize)]
pub struct RuleRow {
    pub address: String,
    #[serde(flatten)]
    pub rule: SenderRule,
}

/// GET /api/sender-rules
pub async fn list(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vec<RuleRow>>, StatusCode> {
    let rows = with_kevy(move |c| rules::list(c, &user))?;
    Ok(Json(
        rows.into_iter()
            .map(|(address, rule)| RuleRow { address, rule })
            .collect(),
    ))
}

#[derive(Debug, serde::Deserialize)]
pub struct RuleBody {
    #[serde(default)]
    pub bucket: Option<String>,
    #[serde(default)]
    pub keep_latest: Option<u32>,
    #[serde(default)]
    pub older_than_days: Option<u32>,
    #[serde(default)]
    pub disposal: Disposal,
}

/// The largest numbers a rule takes: beyond them a rule keeps everything.
const MAX_KEEP: u32 = 10_000;
const MAX_DAYS: u32 = 3650;

#[derive(Debug, serde::Serialize)]
pub struct RuleAnswer {
    /// Conversations the rule let go of when it was saved.
    pub disposed: u32,
}

/// PUT /api/sender-rules/{address} — a retention rule applies at once.
pub async fn put(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(body): Json<RuleBody>,
) -> Result<Json<RuleAnswer>, StatusCode> {
    let sender = rules::normalize(&address);
    let bad_bucket = body.bucket.as_deref().is_some_and(|b| !is_bucket(b));
    let bad_keep = body.keep_latest.is_some_and(|n| n == 0 || n > MAX_KEEP);
    let bad_days = body.older_than_days.is_some_and(|d| d == 0 || d > MAX_DAYS);
    let empty =
        body.bucket.is_none() && body.keep_latest.is_none() && body.older_than_days.is_none();
    if !sender.contains('@') || bad_bucket || bad_keep || bad_days || empty {
        return Err(StatusCode::BAD_REQUEST);
    }
    let rule = SenderRule {
        bucket: body.bucket,
        keep_latest: body.keep_latest,
        older_than_days: body.older_than_days,
        disposal: body.disposal,
        source: "settings".into(),
        created_at: chrono::Utc::now().timestamp(),
    };
    let (who, s, r) = (user.clone(), sender.clone(), rule.clone());
    with_kevy(move |c| rules::put(c, &who, &s, &r))?;
    if !rule.retains() {
        return Ok(Json(RuleAnswer { disposed: 0 }));
    }
    let disposal = match rule.disposal {
        Disposal::Archive => "archive",
        Disposal::Trash => "trash",
    };
    let req = wire::RetainSenderRequest {
        sender,
        keep_latest: rule.keep_latest,
        older_than_days: rule.older_than_days,
        disposal: disposal.into(),
        now: chrono::Utc::now().timestamp(),
    };
    let disposed = state
        .core
        .retain_sender(&user, &req)
        .await
        .map_err(core_err)?
        .disposed;
    Ok(Json(RuleAnswer { disposed }))
}

/// DELETE /api/sender-rules/{address}
pub async fn delete(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let sender = rules::normalize(&address);
    let removed = with_kevy(move |c| rules::delete(c, &user, &sender))?;
    match removed {
        true => Ok(StatusCode::NO_CONTENT),
        false => Err(StatusCode::NOT_FOUND),
    }
}
