//! IMAP imports: how many a user started today, and how far an interrupted
//! one got. Never the password: it lives in the running task's memory only,
//! and an import that stopped is resumed by asking for it again.
//!
//!   `mailrs:imap-import:started:{user}:{yyyymmdd}`  counter, two days' TTL
//!   `mailrs:imap-import:progress:{user}`            hash: `host`, `port`,
//!       `username`, and `uid:{folder}` → the last UID filed from it

use std::collections::HashMap;
use std::io;

use kevy_client::Connection;

/// The most imports a user may start in one UTC day, failed ones included.
pub const STARTS_PER_DAY: i64 = 5;

fn started_key(user: &str, day: &str) -> String {
    format!("mailrs:imap-import:started:{user}:{day}")
}

fn progress_key(user: &str) -> String {
    format!("mailrs:imap-import:progress:{user}")
}

/// Count one start on `day` (`yyyymmdd`, UTC); the count after it.
pub fn count_start(conn: &mut Connection, user: &str, day: &str) -> io::Result<i64> {
    let key = started_key(user, day);
    let n = conn.incr(key.as_bytes())?;
    conn.expire(key.as_bytes(), std::time::Duration::from_secs(2 * 86_400))?;
    Ok(n)
}

/// Where an import stopped: the source, and the last UID filed per folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Progress {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub last_uid: HashMap<String, u32>,
}

/// The progress of `user`'s last unfinished import, if any.
pub fn progress(conn: &mut Connection, user: &str) -> io::Result<Option<Progress>> {
    let flat = conn.hgetall(progress_key(user).as_bytes())?;
    if flat.is_empty() {
        return Ok(None);
    }
    let mut p = Progress::default();
    for pair in flat.chunks_exact(2) {
        let field = String::from_utf8_lossy(&pair[0]);
        let value = String::from_utf8_lossy(&pair[1]).into_owned();
        match field.as_ref() {
            "host" => p.host = value,
            "port" => p.port = value.parse().unwrap_or(0),
            "username" => p.username = value,
            other => {
                if let (Some(folder), Ok(uid)) = (other.strip_prefix("uid:"), value.parse()) {
                    p.last_uid.insert(folder.to_string(), uid);
                }
            }
        }
    }
    Ok(Some(p))
}

/// Begin recording progress against a source, forgetting any other.
pub fn begin(
    conn: &mut Connection,
    user: &str,
    host: &str,
    port: u16,
    username: &str,
) -> io::Result<()> {
    let key = progress_key(user);
    conn.del(&[key.as_bytes()])?;
    conn.hset(
        key.as_bytes(),
        &[
            (b"host" as &[u8], host.as_bytes()),
            (b"port", port.to_string().as_bytes()),
            (b"username", username.as_bytes()),
        ],
    )?;
    Ok(())
}

/// Record that `uid` in `folder` is filed.
pub fn record(conn: &mut Connection, user: &str, folder: &str, uid: u32) -> io::Result<()> {
    let field = format!("uid:{folder}");
    conn.hset(
        progress_key(user).as_bytes(),
        &[(field.as_bytes(), uid.to_string().as_bytes())],
    )?;
    Ok(())
}

/// The import finished: nothing to resume.
pub fn finish(conn: &mut Connection, user: &str) -> io::Result<()> {
    conn.del(&[progress_key(user).as_bytes()])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_round_trips_and_a_new_source_forgets_the_old() {
        let mut c = Connection::connect("mem://imap-import-progress").unwrap();
        let u = "a@x.test";
        assert_eq!(progress(&mut c, u).unwrap(), None);
        begin(&mut c, u, "imap.old.test", 993, "a@old.test").unwrap();
        record(&mut c, u, "INBOX", 41).unwrap();
        record(&mut c, u, "[Gmail]/All Mail", 7).unwrap();
        record(&mut c, u, "INBOX", 42).unwrap();
        let p = progress(&mut c, u).unwrap().unwrap();
        assert_eq!(
            (p.host.as_str(), p.port, p.username.as_str()),
            ("imap.old.test", 993, "a@old.test")
        );
        assert_eq!(p.last_uid.get("INBOX"), Some(&42));
        assert_eq!(p.last_uid.get("[Gmail]/All Mail"), Some(&7));
        begin(&mut c, u, "imap.other.test", 143, "a").unwrap();
        assert!(progress(&mut c, u).unwrap().unwrap().last_uid.is_empty());
        finish(&mut c, u).unwrap();
        assert_eq!(progress(&mut c, u).unwrap(), None);
    }

    #[test]
    fn starts_are_counted_per_day() {
        let mut c = Connection::connect("mem://imap-import-starts").unwrap();
        assert_eq!(count_start(&mut c, "a@x", "20261101").unwrap(), 1);
        assert_eq!(count_start(&mut c, "a@x", "20261101").unwrap(), 2);
        assert_eq!(count_start(&mut c, "a@x", "20261102").unwrap(), 1);
        assert_eq!(count_start(&mut c, "b@x", "20261101").unwrap(), 1);
    }
}
