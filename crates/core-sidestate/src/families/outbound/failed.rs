//! Jobs that failed for good: the sender records each one as it gives up,
//! and the admin queue page and overview read them back.
//!
//! The job itself is dropped once it fails, so this record is all that
//! says what it was. Records written before sender and recipient were kept
//! have them empty.

use std::io;

use super::{FAILED_SET, failed_audit_key};

/// One job that failed for good.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub id: String,
    /// unix seconds; 0 when the record has no time
    pub failed_at: i64,
    pub reason: String,
    pub sender: String,
    pub recipient: String,
}

/// Record that job `id` failed for good.
pub fn record_failure(c: &mut kevy_client::Connection, f: &Failure) -> io::Result<()> {
    c.sadd(FAILED_SET, &[f.id.as_bytes()])?;
    let at = f.failed_at.to_string();
    c.hset(
        failed_audit_key(&f.id).as_bytes(),
        &[
            (b"failed_at" as &[u8], at.as_bytes()),
            (b"reason", f.reason.as_bytes()),
            (b"sender", f.sender.as_bytes()),
            (b"recipient", f.recipient.as_bytes()),
        ],
    )?;
    Ok(())
}

/// Every job that failed for good, newest first.
pub fn failures(c: &mut kevy_client::Connection) -> io::Result<Vec<Failure>> {
    let mut out = Vec::new();
    for id in c.smembers(FAILED_SET)? {
        let id = String::from_utf8_lossy(&id).into_owned();
        let flat = c.hgetall(failed_audit_key(&id).as_bytes())?;
        let mut f = Failure {
            id,
            failed_at: 0,
            reason: String::new(),
            sender: String::new(),
            recipient: String::new(),
        };
        for pair in flat.chunks_exact(2) {
            let value = String::from_utf8_lossy(&pair[1]).into_owned();
            match pair[0].as_slice() {
                b"failed_at" => f.failed_at = value.parse().unwrap_or(0),
                b"reason" => f.reason = value,
                b"sender" => f.sender = value,
                b"recipient" => f.recipient = value,
                _ => {}
            }
        }
        out.push(f);
    }
    out.sort_by(|a, b| b.failed_at.cmp(&a.failed_at).then_with(|| b.id.cmp(&a.id)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_read_back_newest_first_including_old_records() {
        let mut c = kevy_client::Connection::connect("mem://outbound-failures").unwrap();
        let f = |id: &str, at: i64| Failure {
            id: id.into(),
            failed_at: at,
            reason: format!("550 for {id}"),
            sender: "a@x.test".into(),
            recipient: "b@y.test".into(),
        };
        record_failure(&mut c, &f("1", 100)).unwrap();
        record_failure(&mut c, &f("2", 300)).unwrap();
        // written before sender and recipient were kept
        c.sadd(FAILED_SET, &[b"3"]).unwrap();
        c.hset(
            failed_audit_key("3").as_bytes(),
            &[(b"failed_at" as &[u8], b"200"), (b"reason", b"old")],
        )
        .unwrap();
        let got = failures(&mut c).unwrap();
        let ids: Vec<&str> = got.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, vec!["2", "3", "1"]);
        assert_eq!(got[0], f("2", 300));
        assert_eq!(got[1].sender, "", "an old record has no sender");
        assert_eq!(got[1].reason, "old");
    }
}
