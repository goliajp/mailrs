//! The outbound queue as the admin page sees it, and the retry action.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use mailrs_core_sidestate::families::outbound::{INFLIGHT, PENDING_IDX, RETRY_IDX, job_key};

use super::now_secs;
use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::{require_admin, with_kevy};

/// Up to 100 ids each of queued, waiting between attempts and in flight,
/// one row per job, without the message body.
pub(crate) fn queue_rows(
    c: &mut kevy_client::Connection,
) -> std::io::Result<Vec<serde_json::Value>> {
    let lists = [
        ("pending", c.lrange(PENDING_IDX, 0, 99)?),
        ("retrying", c.zrange(RETRY_IDX, 0, 99)?),
        ("inflight", c.lrange(INFLIGHT, 0, 99)?),
    ];
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    for (label, ids) in lists {
        for id in ids {
            let id = String::from_utf8_lossy(&id).into_owned();
            if !seen.insert(id.clone()) {
                continue;
            }
            let Some(blob) = c.hget(job_key(&id).as_bytes(), b"blob")? else {
                continue;
            };
            let Ok(serde_json::Value::Object(mut row)) = serde_json::from_slice(&blob) else {
                continue;
            };
            row.remove("message_data_b64");
            row.insert("status".into(), label.into());
            items.push(serde_json::Value::Object(row));
        }
    }
    Ok(items)
}

pub async fn list_admin_queue() -> Result<Json<serde_json::Value>, StatusCode> {
    let items = with_kevy(queue_rows)?;
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RetryOutcome {
    Released,
    NotFound,
    /// The job exists but is not waiting between attempts: it is queued,
    /// in flight, or finished.
    NotWaiting,
}

/// Release a job waiting on `retry-idx` onto `pending-idx` now.
pub(crate) fn retry_now(
    c: &mut kevy_client::Connection,
    id: i64,
    now: i64,
) -> std::io::Result<RetryOutcome> {
    let id_str = id.to_string();
    let key = job_key(&id_str);
    if c.exists(&[key.as_bytes()])? == 0 {
        return Ok(RetryOutcome::NotFound);
    }
    if c.zscore(RETRY_IDX, id_str.as_bytes())?.is_none() {
        return Ok(RetryOutcome::NotWaiting);
    }
    // the sender re-parks a job whose last attempt is younger than its
    // backoff floor, so drop the timestamp or the release does nothing
    if let Some(blob) = c.hget(key.as_bytes(), b"blob")?
        && let Ok(serde_json::Value::Object(mut env)) = serde_json::from_slice(&blob)
    {
        env.remove("last_attempt_at");
        let payload = serde_json::Value::Object(env).to_string();
        c.hset(
            key.as_bytes(),
            &[
                (b"blob" as &[u8], payload.as_bytes()),
                (b"updated_at", now.to_string().as_bytes()),
            ],
        )?;
    }
    // pending first, then the zset, the order the sender's own sweep uses
    c.lpush(PENDING_IDX, &[id_str.as_bytes()])?;
    c.zrem(RETRY_IDX, &[id_str.as_bytes()])?;
    Ok(RetryOutcome::Released)
}

/// POST /api/queue/{id}/retry — send a job that is waiting for its next
/// attempt now. The route sits outside `/api/admin/`, so the admin check
/// is made here.
pub async fn queue_retry(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    require_admin(&state, &user).await?;
    let now = now_secs();
    match with_kevy(move |c| retry_now(c, id, now))? {
        RetryOutcome::Released => Ok(StatusCode::NO_CONTENT),
        RetryOutcome::NotFound => Err(StatusCode::NOT_FOUND),
        RetryOutcome::NotWaiting => Err(StatusCode::CONFLICT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(name: &str) -> kevy_client::Connection {
        kevy_client::Connection::connect(&format!("mem://{name}")).unwrap()
    }

    fn put_job(c: &mut kevy_client::Connection, id: &str, blob: &str) {
        c.hset(
            job_key(id).as_bytes(),
            &[(b"blob" as &[u8], blob.as_bytes()), (b"state", b"pending")],
        )
        .unwrap();
    }

    #[test]
    fn the_list_labels_each_state_once_and_drops_the_body() {
        let mut c = conn("queue-rows");
        put_job(
            &mut c,
            "1",
            r#"{"id":1,"recipient":"a@b.c","message_data_b64":"QUJD"}"#,
        );
        put_job(
            &mut c,
            "2",
            r#"{"id":2,"recipient":"d@e.f","message_data_b64":"QUJD"}"#,
        );
        c.lpush(PENDING_IDX, &[b"1", b"1"]).unwrap();
        c.zadd(RETRY_IDX, &[(5.0, b"2" as &[u8])]).unwrap();
        let rows = queue_rows(&mut c).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (rows[0]["id"].as_i64(), rows[0]["status"].as_str()),
            (Some(1), Some("pending"))
        );
        assert_eq!(
            (rows[1]["id"].as_i64(), rows[1]["status"].as_str()),
            (Some(2), Some("retrying"))
        );
        assert!(rows.iter().all(|r| r.get("message_data_b64").is_none()));
    }

    #[test]
    fn retry_releases_only_a_waiting_job_and_says_when_there_is_none() {
        let mut c = conn("queue-retry");
        assert_eq!(retry_now(&mut c, 9, 100).unwrap(), RetryOutcome::NotFound);

        put_job(&mut c, "3", r#"{"id":3,"attempts":2,"last_attempt_at":90}"#);
        c.lpush(PENDING_IDX, &[b"3"]).unwrap();
        assert_eq!(retry_now(&mut c, 3, 100).unwrap(), RetryOutcome::NotWaiting);

        put_job(&mut c, "4", r#"{"id":4,"attempts":2,"last_attempt_at":90}"#);
        c.zadd(RETRY_IDX, &[(500.0, b"4" as &[u8])]).unwrap();
        assert_eq!(retry_now(&mut c, 4, 100).unwrap(), RetryOutcome::Released);
        assert!(c.zscore(RETRY_IDX, b"4").unwrap().is_none());
        assert_eq!(c.lrange(PENDING_IDX, 0, 0).unwrap(), vec![b"4".to_vec()]);
        let blob = c.hget(job_key("4").as_bytes(), b"blob").unwrap().unwrap();
        let env: serde_json::Value = serde_json::from_slice(&blob).unwrap();
        assert!(env.get("last_attempt_at").is_none());
        assert_eq!(env["attempts"], 2);
    }
}
