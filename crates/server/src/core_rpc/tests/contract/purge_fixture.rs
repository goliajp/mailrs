//! What the account purge contract gives each core before the delete: two
//! messages, one dave alone reads and one he shares with alice, and three
//! of dave's jobs in each core's queue.

use super::harness::{Cores, FASTCORE_KEVY, PG_CORE_KEVY};

pub(super) const DAVE: &str = "dave@example.test";
pub(super) const DAVE_ONLY: &str = "dave1@outside.example";
pub(super) const SHARED: &str = "dave2@outside.example";
/// A send time far ahead.
pub(super) const LATER: i64 = 4_000_000_000;

pub(super) fn mail(mid: &str) -> Vec<u8> {
    format!(
        "From: Carol <carol@outside.example>\r\nTo: dave@example.test\r\n\
         Subject: before he left\r\nMessage-ID: <{mid}>\r\n\
         Date: Tue, 5 Aug 2025 09:00:00 +0900\r\n\r\n\
         hello again, here is the long note about the order and the invoice \
         that we talked about on the phone last week\r\n"
    )
    .into_bytes()
}

/// Whether each core still holds `mid`'s template fingerprint.
pub(super) async fn fingerprints(c: &Cores, mid: &str) -> (bool, bool) {
    let fc = c
        .fastcore_state
        .mailbox
        .template_of(mid)
        .expect("fastcore fingerprint")
        .is_some();
    let (n,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM message_templates WHERE message_id = $1")
            .bind(mid)
            .fetch_one(&c.pg_core_state.pool)
            .await
            .expect("pg fingerprint");
    (fc, n == 1)
}

/// Dave's mail in one core's queue: one send scheduled for later, one
/// already released for delivery, one released and waiting for a retry.
pub(super) struct Queued {
    pub(super) url: &'static str,
    pub(super) scheduled: i64,
    pub(super) released: i64,
    pub(super) retrying: i64,
}

pub(super) fn queue() -> Vec<Queued> {
    use mailrs_core_sidestate::families::outbound as ob;
    [FASTCORE_KEVY, PG_CORE_KEVY]
        .into_iter()
        .map(|url| {
            let mut s = store(url);
            // the job rows as the enqueue writes them (its pipeline is not
            // in the in-process store), then each in its index
            let mut job = || {
                let id = s.incr(ob::COUNTER).expect("id");
                let blob = serde_json::json!({ "id": id, "sender": DAVE,
                    "recipient": "erin@outside.example" })
                .to_string();
                s.hset(
                    ob::job_key(id).as_bytes(),
                    &[
                        (b"state" as &[u8], b"pending" as &[u8]),
                        (b"blob", blob.as_bytes()),
                        (b"updated_at", b"1790000000"),
                    ],
                )
                .expect("job");
                id
            };
            let (scheduled, released, retrying) = (job(), job(), job());
            let later = LATER as f64;
            s.zadd(
                ob::SCHEDULED_IDX,
                &[(later, scheduled.to_string().as_bytes())],
            )
            .expect("scheduled");
            s.lpush(ob::PENDING_IDX, &[released.to_string().as_bytes()])
                .expect("pending");
            s.zadd(ob::RETRY_IDX, &[(later, retrying.to_string().as_bytes())])
                .expect("retry");
            Queued {
                url,
                scheduled,
                released,
                retrying,
            }
        })
        .collect()
}

/// A job's state, and its scores in the scheduled and the retry index.
pub(super) fn job(url: &str, id: i64) -> (Option<String>, Option<f64>, Option<f64>) {
    use mailrs_core_sidestate::families::outbound as ob;
    let mut s = store(url);
    let state = s
        .hget(ob::job_key(id).as_bytes(), b"state")
        .expect("state")
        .map(|v| String::from_utf8_lossy(&v).into_owned());
    let id = id.to_string();
    (
        state,
        s.zscore(ob::SCHEDULED_IDX, id.as_bytes()).expect("zscore"),
        s.zscore(ob::RETRY_IDX, id.as_bytes()).expect("zscore"),
    )
}

/// What was released for delivery stays as it was, whatever happens to
/// the account.
pub(super) fn released_untouched(q: &Queued) {
    let pending = Some("pending".to_string());
    assert_eq!(job(q.url, q.released).0, pending, "{}", q.url);
    assert_eq!(
        job(q.url, q.retrying),
        (pending, None, Some(LATER as f64)),
        "{}",
        q.url
    );
}

pub(super) fn store(url: &str) -> kevy_client::Connection {
    kevy_client::Connection::connect(url).expect("side state")
}
