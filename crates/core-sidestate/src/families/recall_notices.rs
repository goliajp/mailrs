//! What a reader is told when mail they already had is taken back into
//! Review: one entry per conversation, newest first, kept a few hundred
//! deep. The core that re-judged the mail writes; the web tier reads it to
//! show a line where the conversation used to be.
//!
//! ```text
//! mailrs:recalled:{user}   list   JSON entries, newest first
//! ```

use serde::{Deserialize, Serialize};

/// Entries kept per reader.
pub const KEPT: i64 = 200;

/// How long the line stays where the conversation was.
pub const SHOWN_SECS: i64 = 7 * 86_400;

/// One conversation taken back, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recalled {
    pub thread_id: String,
    pub subject: String,
    /// The rules that now hold it, by id.
    pub rules: Vec<String>,
    /// The observation that sent it back through the rules.
    pub observation: String,
    /// Unix seconds.
    pub at: i64,
}

pub fn key(user: &str) -> String {
    format!("mailrs:recalled:{}", user.to_ascii_lowercase())
}

/// Tell `user` about one conversation taken back.
///
/// # Errors
/// The store could not be written.
pub fn push(conn: &mut kevy_client::Connection, user: &str, r: &Recalled) -> std::io::Result<()> {
    let json = serde_json::to_vec(r).map_err(std::io::Error::other)?;
    let k = key(user);
    let len = conn.lpush(k.as_bytes(), &[json.as_slice()])? as i64;
    // no LTRIM in this client: the oldest go from the tail
    if len > KEPT {
        conn.rpop(k.as_bytes(), (len - KEPT) as usize)?;
    }
    Ok(())
}

/// The entries of the last [`SHOWN_SECS`] before `now`, newest first; one
/// per conversation, the newest.
///
/// # Errors
/// The store could not be read.
pub fn recent(
    conn: &mut kevy_client::Connection,
    user: &str,
    now: i64,
) -> std::io::Result<Vec<Recalled>> {
    let raw = conn.lrange(key(user).as_bytes(), 0, KEPT - 1)?;
    let mut seen = std::collections::HashSet::new();
    Ok(raw
        .iter()
        .filter_map(|r| serde_json::from_slice::<Recalled>(r).ok())
        .filter(|r| now - r.at <= SHOWN_SECS)
        .filter(|r| seen.insert(r.thread_id.clone()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(tid: &str, at: i64) -> Recalled {
        Recalled {
            thread_id: tid.into(),
            subject: "s".into(),
            rules: vec!["executable-attachment".into()],
            observation: "r1".into(),
            at,
        }
    }

    #[test]
    fn the_newest_are_kept_one_per_conversation_for_a_week() {
        let mut c = kevy_client::Connection::connect("mem://recall-notices").unwrap();
        let now = 2_000_000_000;
        push(&mut c, "Bob@x.com", &entry("old", now - SHOWN_SECS - 1)).unwrap();
        assert!(
            recent(&mut c, "bob@x.com", now).unwrap().is_empty(),
            "a week old"
        );
        for n in 0..KEPT + 5 {
            push(&mut c, "bob@x.com", &entry(&format!("t{n}"), now)).unwrap();
        }
        push(&mut c, "bob@x.com", &entry("t200", now)).unwrap();
        assert_eq!(c.llen(key("bob@x.com").as_bytes()).unwrap() as i64, KEPT);
        let shown = recent(&mut c, "bob@x.com", now).unwrap();
        assert_eq!(shown[0].thread_id, "t200", "newest first");
        assert_eq!(shown.iter().filter(|r| r.thread_id == "t200").count(), 1);
        assert_eq!(shown.len() as i64, KEPT - 1, "one repeat folded away");
    }
}
