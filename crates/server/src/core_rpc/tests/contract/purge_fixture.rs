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

/// The message dave scheduled and whose time passes while he is deleted,
/// with two attachments, which the draft it becomes carries.
pub(super) const LAPSING: &[u8] = b"From: Dave <dave@example.test>\r\n\
To: Erin <erin@outside.example>\r\nCc: fay@outside.example\r\n\
Subject: =?UTF-8?B?44GK55+l44KJ44Gb?=\r\nMessage-ID: <lapsing@example.test>\r\n\
MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n\
--b1\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nsee you on monday\r\n\
--b1\r\nContent-Type: application/pdf; name=\"agenda.pdf\"\r\n\
Content-Disposition: attachment; filename=\"agenda.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n\
--b1\r\nContent-Type: image/png; name=\"map.png\"\r\n\
Content-Disposition: attachment; filename=\"map.png\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\niVBORw0KGgo=\r\n\
--b1--\r\n";

/// Dave's mail in one core's queue: two sends scheduled for later (the
/// second, to erin, fay and a blind gus, lapses while he is deleted), one
/// already released for delivery, one released and waiting for a retry.
pub(super) struct Queued {
    pub(super) url: &'static str,
    pub(super) scheduled: i64,
    pub(super) lapsing: Vec<i64>,
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
            let mut job = |rcpt: &str, send_id: &str, raw: &[u8]| {
                use base64::Engine as _;
                let id = s.incr(ob::COUNTER).expect("id");
                let data = base64::engine::general_purpose::STANDARD.encode(raw);
                let blob = serde_json::json!({ "id": id, "sender": DAVE,
                    "recipient": rcpt, "send_id": send_id, "message_data_b64": data })
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
            let erin = "erin@outside.example";
            let scheduled = job(erin, "s1@example.test", b"Subject: s1\r\n\r\nx\r\n");
            let released = job(erin, "s2@example.test", b"Subject: s2\r\n\r\nx\r\n");
            let retrying = job(erin, "s3@example.test", b"Subject: s3\r\n\r\nx\r\n");
            let lapsing: Vec<i64> = [erin, "fay@outside.example", "gus@outside.example"]
                .into_iter()
                .map(|r| job(r, "lapsing@example.test", LAPSING))
                .collect();
            let later = LATER as f64;
            for id in std::iter::once(scheduled).chain(lapsing.iter().copied()) {
                s.zadd(ob::SCHEDULED_IDX, &[(later, id.to_string().as_bytes())])
                    .expect("scheduled");
            }
            s.lpush(ob::PENDING_IDX, &[released.to_string().as_bytes()])
                .expect("pending");
            s.zadd(ob::RETRY_IDX, &[(later, retrying.to_string().as_bytes())])
                .expect("retry");
            Queued {
                url,
                scheduled,
                lapsing,
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

/// The lapsing send's time passes while dave is deleted: what the delete
/// withheld now waits for a time gone by.
pub(super) fn lapse(q: &Queued) {
    use mailrs_core_sidestate::families::outbound as ob;
    let mut s = store(q.url);
    for id in &q.lapsing {
        s.hset(
            ob::withheld_key(DAVE).as_bytes(),
            &[(id.to_string().as_bytes(), b"1790000000".as_slice())],
        )
        .expect("lapse");
    }
}

/// Whether job `id` can still be sent: its row, or its id in any index.
pub(super) fn queued(url: &str, id: i64) -> bool {
    use mailrs_core_sidestate::families::outbound as ob;
    let mut s = store(url);
    let m = id.to_string();
    s.exists(&[ob::job_key(id).as_bytes()]).expect("exists") == 1
        || s.zscore(ob::SCHEDULED_IDX, m.as_bytes())
            .expect("zscore")
            .is_some()
        || s.zscore(ob::RETRY_IDX, m.as_bytes())
            .expect("zscore")
            .is_some()
        || s.lrange(ob::PENDING_IDX, 0, -1)
            .expect("pending")
            .iter()
            .any(|p| p == m.as_bytes())
}

/// Dave's drafts in one core's shared store.
pub(super) fn drafts(url: &str) -> Vec<serde_json::Value> {
    let mut s = store(url);
    let key = mailrs_core_sidestate::families::prefs::drafts_key(DAVE);
    s.hgetall(key.as_bytes())
        .expect("drafts")
        .chunks(2)
        .map(|p| serde_json::from_slice(&p[1]).expect("draft"))
        .collect()
}

/// The details of one core's audit rows with `action`.
pub(super) fn audit_details(url: &str, action: &str) -> Vec<String> {
    use mailrs_core_sidestate::families::audit::AuditFact;
    store(url)
        .lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .expect("audit")
        .iter()
        .map(|r| serde_json::from_slice::<AuditFact>(r).expect("fact"))
        .filter(|f| f.action == action)
        .map(|f| f.detail)
        .collect()
}
