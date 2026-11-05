//! Queueing an ops alert as mail to the operator addresses: the own path
//! of the ops daemon's alerts, used by both cores' `/v1/ops/alerts` and by
//! webapi when the daemon itself goes silent.
//!
//! Recipients come only from the configuration passed in; nothing here
//! takes an address from the caller.

use base64::Engine as _;
use mailrs_core_api::method::ops_alert::{OpsAlertWire, OpsOperators};

use super::outbound::{FreshPending, write_ops_alert_pending};

/// The message, plain text, base64 so any language needs no 8BITMIME.
pub fn message(ops: &OpsOperators, alert: &OpsAlertWire, now: i64) -> Vec<u8> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let body = b64.encode(alert.text.as_bytes());
    let lines: Vec<&str> = body
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();
    let domain = ops.from.rsplit_once('@').map_or("localhost", |(_, d)| d);
    let date =
        chrono::DateTime::from_timestamp(now, 0).map_or_else(String::new, |d| d.to_rfc2822());
    // the subject is ours to write, but it came over the wire: one line
    let subject: String = alert.subject.chars().filter(|c| !c.is_control()).collect();
    let id: String = alert
        .id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    format!(
        "From: {}\r\nTo: {}\r\nSubject: =?utf-8?B?{}?=\r\nDate: {date}\r\nMessage-ID: <ops-alert.{id}@{domain}>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n",
        ops.from,
        ops.to.join(", "),
        b64.encode(subject.as_bytes()),
        lines.join("\r\n"),
    )
    .into_bytes()
}

/// Queue `alert` to each operator address on the shared store; the jobs
/// queued.
pub fn submit(
    conn: &mut kevy_client::Connection,
    ops: &OpsOperators,
    alert: &OpsAlertWire,
    now: i64,
) -> std::io::Result<usize> {
    let data = base64::engine::general_purpose::STANDARD.encode(message(ops, alert, now));
    for rcpt in &ops.to {
        let job = FreshPending {
            sender: &ops.from,
            recipient: rcpt,
            message_data_base64: &data,
            scheduled_at: None,
            original_sender: None,
            send_id: None,
            account: None,
        };
        write_ops_alert_pending(conn, &job, &alert.id, now)?;
    }
    Ok(ops.to.len())
}

/// [`submit`] on the store `core` reaches.
pub fn submit_for<C: crate::NetKevy>(
    core: &C,
    ops: &OpsOperators,
    alert: &OpsAlertWire,
) -> Result<usize, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let mut conn = core
        .net_conn()
        .ok_or_else(|| "the shared store cannot be reached".to_string())?;
    submit(&mut conn, ops, alert, now).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::families::outbound::{OPS_ALERT_ID, PENDING_IDX, job_key};

    fn ops() -> OpsOperators {
        OpsOperators {
            from: "ops-alerts@example.test".into(),
            to: vec!["oncall@example.test".into(), "second@example.test".into()],
        }
    }

    /// Against a kevy-server: jobs are written with a pipeline, which the
    /// in-process store does not run.
    #[tokio::test]
    async fn an_alert_is_one_job_per_operator_carrying_its_id() {
        let (_kevy, url) = mailrs_test_docker::kevy::kevy_server().await;
        let mut c = kevy_client::Connection::connect(&url).unwrap();
        let alert = OpsAlertWire {
            id: "1800000000-7".into(),
            subject: "有信被隔离\r\nBcc: x@evil.test".into(),
            text: "第一步".into(),
        };
        assert_eq!(submit(&mut c, &ops(), &alert, 1_800_000_000).unwrap(), 2);
        let ids = c.lrange(PENDING_IDX, 0, -1).unwrap();
        assert_eq!(ids.len(), 2);
        let mut to = Vec::new();
        for id in ids {
            let id = String::from_utf8(id).unwrap();
            let blob = c.hget(job_key(id).as_bytes(), b"blob").unwrap().unwrap();
            let v: serde_json::Value = serde_json::from_slice(&blob).unwrap();
            assert_eq!(v[OPS_ALERT_ID], "1800000000-7");
            assert_eq!(v["sender"], "ops-alerts@example.test");
            to.push(v["recipient"].as_str().unwrap().to_string());
            let raw = base64::engine::general_purpose::STANDARD
                .decode(v["message_data_b64"].as_str().unwrap())
                .unwrap();
            let head = String::from_utf8_lossy(&raw);
            assert!(
                !head.contains("Bcc:"),
                "the subject stays one header: {head}"
            );
        }
        to.sort();
        assert_eq!(to, ["oncall@example.test", "second@example.test"]);
    }
}
