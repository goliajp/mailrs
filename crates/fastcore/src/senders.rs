//! A reader's rules for one sender, on this store: moving a sender's
//! conversations, putting them back, and keeping only what a retention
//! rule keeps (`mailrs_intake::sender_ops` decides; this changes the rows).

use std::io;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mailrs_core_api::method::senders as wire;
use mailrs_core_sidestate::families::sender_rules::{Disposal, Moved, SenderRule};
use mailrs_intake::sender_ops::{self, SenderStore, SenderThread};
use mailrs_mailbox_kevy::scope::{Bucket, bucket_of};

use crate::FastcoreState;

pub(crate) struct Senders<'a>(pub(crate) &'a FastcoreState);

fn bucket_named(name: &str) -> io::Result<Bucket> {
    match name {
        "inbox" => Ok(Bucket::Inbox),
        "notifications" => Ok(Bucket::Notifications),
        "promotions" => Ok(Bucket::Promotions),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("not a bucket a sender's mail can go to: {other}"),
        )),
    }
}

impl SenderStore for Senders<'_> {
    fn sender_threads(&self, user: &str, sender: &str) -> io::Result<Vec<SenderThread>> {
        let mut out = Vec::new();
        for tid in self.0.mailbox.sender_thread_ids(user, sender)? {
            let Some(row) = self.0.mailbox.get_thread_for_user(user, &tid)? else {
                continue;
            };
            out.push(SenderThread {
                bucket: bucket_of(&row.category).name().to_string(),
                latest_date: row.latest_date,
                archived: row.archived,
                trashed: row.trashed_at > 0,
                held: row.quarantined,
                starred: row.starred,
                pinned: row.pinned,
                thread_id: tid,
            });
        }
        Ok(out)
    }

    fn set_bucket(&self, user: &str, thread_id: &str, bucket: &str) -> io::Result<()> {
        self.0
            .mailbox
            .set_bucket(user, thread_id, bucket_named(bucket)?)?;
        Ok(())
    }

    fn dispose(&self, user: &str, thread_id: &str, how: Disposal, now: i64) -> io::Result<()> {
        match how {
            Disposal::Archive => self.0.mailbox.set_archived(user, thread_id, true)?,
            Disposal::Trash => self.0.mailbox.set_trashed(user, thread_id, true, now)?,
        };
        Ok(())
    }
}

fn moved_wire(m: Vec<Moved>) -> Vec<wire::MovedWire> {
    m.into_iter()
        .map(|m| wire::MovedWire {
            thread_id: m.thread_id,
            from_bucket: m.from_bucket,
        })
        .collect()
}

/// Run `f` on a blocking thread; a bad bucket is the caller's mistake.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> Result<T, StatusCode> {
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) if e.kind() == io::ErrorKind::InvalidInput => Err(StatusCode::BAD_REQUEST),
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "sender rule action failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
        Err(e) => {
            tracing::warn!(error = %e, "sender rule action panicked");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// `POST /v1/users/{user}/senders:reclassify`
pub(crate) async fn reclassify(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<wire::ReclassifySenderRequest>,
) -> Result<Json<wire::ReclassifySenderResponse>, StatusCode> {
    bucket_named(&req.bucket).map_err(|_| StatusCode::BAD_REQUEST)?;
    let moved =
        blocking(move || sender_ops::reclassify(&Senders(&state), &user, &req.sender, &req.bucket))
            .await?;
    Ok(Json(wire::ReclassifySenderResponse {
        moved: moved_wire(moved),
    }))
}

/// `POST /v1/users/{user}/senders:restore`
pub(crate) async fn restore(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<wire::RestoreSenderRequest>,
) -> Result<Json<wire::RestoreSenderResponse>, StatusCode> {
    let moved: Vec<Moved> = req
        .moved
        .into_iter()
        .map(|m| Moved {
            thread_id: m.thread_id,
            from_bucket: m.from_bucket,
        })
        .collect();
    let restored = blocking(move || {
        sender_ops::restore(&Senders(&state), &user, &req.sender, &req.to_bucket, &moved)
    })
    .await?;
    Ok(Json(wire::RestoreSenderResponse { restored }))
}

/// The rule a retain request describes.
pub(crate) fn rule_of(req: &wire::RetainSenderRequest) -> Result<SenderRule, StatusCode> {
    let disposal = match req.disposal.as_str() {
        "archive" => Disposal::Archive,
        "trash" => Disposal::Trash,
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    Ok(SenderRule {
        keep_latest: req.keep_latest,
        older_than_days: req.older_than_days,
        disposal,
        ..Default::default()
    })
}

/// `POST /v1/users/{user}/senders:retain`
pub(crate) async fn retain(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<wire::RetainSenderRequest>,
) -> Result<Json<wire::RetainSenderResponse>, StatusCode> {
    let rule = rule_of(&req)?;
    let disposed =
        blocking(move || sender_ops::retain(&Senders(&state), &user, &req.sender, &rule, req.now))
            .await?;
    Ok(Json(wire::RetainSenderResponse { disposed }))
}

/// After a delivery from `from`: a "keep the newest N" rule may now let go
/// of one.
pub(crate) fn after_delivery(state: &FastcoreState, user: &str, from: &str) {
    let now = chrono::Utc::now().timestamp();
    sender_ops::after_delivery(&Senders(state), state.net_conn().as_mut(), user, from, now);
}

/// The age rules, hourly; six hours apart while nothing is due, which costs
/// one read of the set of readers with such a rule.
pub(crate) fn spawn_retention(state: Arc<FastcoreState>) {
    const BASE: Duration = Duration::from_secs(3600);
    const MAX: Duration = Duration::from_secs(6 * 3600);
    tokio::spawn(async move {
        let mut wait = BASE;
        loop {
            tokio::time::sleep(wait).await;
            let st = state.clone();
            let gone = tokio::task::spawn_blocking(move || {
                let mut conn = st.net_conn()?;
                let now = chrono::Utc::now().timestamp();
                sender_ops::sweep(&Senders(&st), &mut conn, now)
                    .map_err(|e| tracing::warn!(error = %e, "sender retention sweep failed"))
                    .ok()
            })
            .await
            .ok()
            .flatten()
            .unwrap_or(0);
            wait = match gone {
                0 => (wait * 2).min(MAX),
                n => {
                    tracing::info!(disposed = n, "sender retention: conversations let go");
                    BASE
                }
            };
        }
    });
}
