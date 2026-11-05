//! What a reader is told when mail they already had is taken back into
//! Review: one entry per round that took some of theirs (one observation's
//! re-judge, or one report), naming every conversation it took, newest
//! first, kept a few hundred deep. The core that took the mail writes; the
//! web tier reads it to show a line where the conversations used to be.
//!
//! ```text
//! mailrs:recalled:{user}   list   JSON entries, newest first
//! ```

use serde::{Deserialize, Serialize};

/// Entries kept per reader.
pub const KEPT: i64 = 200;

/// How long the line stays where the conversations were.
pub const SHOWN_SECS: i64 = 7 * 86_400;

/// The conversations one round took back, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recalled {
    pub thread_ids: Vec<String>,
    /// The rules that now hold them, by id.
    pub rules: Vec<String>,
    /// The observation (or report batch) that sent them back through the
    /// rules.
    pub observation: String,
    /// Unix seconds.
    pub at: i64,
}

pub fn key(user: &str) -> String {
    format!("mailrs:recalled:{}", user.to_ascii_lowercase())
}

/// Tell `user` about the conversations one round took back.
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

/// The entries of the last [`SHOWN_SECS`] before `now`, newest first. A
/// conversation is named only by the newest entry that took it, and an
/// entry left naming none is dropped.
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
        .filter_map(|mut r| {
            r.thread_ids.retain(|t| seen.insert(t.clone()));
            (!r.thread_ids.is_empty()).then_some(r)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(ids: &[&str], observation: &str, at: i64) -> Recalled {
        Recalled {
            thread_ids: ids.iter().map(|s| s.to_string()).collect(),
            rules: vec!["executable-attachment".into()],
            observation: observation.into(),
            at,
        }
    }

    #[test]
    fn the_newest_are_kept_one_per_conversation_for_a_week() {
        let mut c = kevy_client::Connection::connect("mem://recall-notices").unwrap();
        let now = 2_000_000_000;
        push(
            &mut c,
            "Bob@x.com",
            &entry(&["old"], "r1", now - SHOWN_SECS - 1),
        )
        .unwrap();
        assert!(
            recent(&mut c, "bob@x.com", now).unwrap().is_empty(),
            "a week old"
        );
        for n in 0..KEPT + 5 {
            push(&mut c, "bob@x.com", &entry(&[&format!("t{n}")], "r1", now)).unwrap();
        }
        push(&mut c, "bob@x.com", &entry(&["t200"], "r1", now)).unwrap();
        assert_eq!(c.llen(key("bob@x.com").as_bytes()).unwrap() as i64, KEPT);
        let shown = recent(&mut c, "bob@x.com", now).unwrap();
        assert_eq!(shown[0].thread_ids, ["t200"], "newest first");
        let named = |tid: &str| {
            shown
                .iter()
                .filter(|r| r.thread_ids.iter().any(|t| t == tid))
                .count()
        };
        assert_eq!(named("t200"), 1);
        assert_eq!(shown.len() as i64, KEPT - 1, "one repeat folded away");
    }

    #[test]
    fn a_round_is_one_entry_and_a_newer_round_names_its_conversations() {
        let mut c = kevy_client::Connection::connect("mem://recall-notices-rounds").unwrap();
        let now = 2_000_000_000;
        push(&mut c, "bob@x.com", &entry(&["a", "b", "c"], "r1", now)).unwrap();
        push(&mut c, "bob@x.com", &entry(&["c"], "r2", now)).unwrap();
        push(&mut c, "bob@x.com", &entry(&["c"], "r3", now)).unwrap();
        let shown = recent(&mut c, "bob@x.com", now).unwrap();
        assert_eq!(shown.len(), 2, "{shown:?}");
        assert_eq!(shown[0].observation, "r3");
        assert_eq!(shown[0].thread_ids, ["c"]);
        assert_eq!(shown[1].observation, "r1");
        assert_eq!(shown[1].thread_ids, ["a", "b"]);
    }
}
