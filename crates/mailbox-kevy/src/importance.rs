//! Thread-level importance verdict write path.
//!
//! Importance is a **derivation**, not a fact: it is recomputed from the
//! message plus the sender relationship every time a message arrives, and
//! can be rebuilt from scratch at any point. It therefore lives outside
//! the atomic arrival block — a reader briefly seeing the previous
//! verdict costs nothing, whereas threading two more fields through all
//! fourteen `MessageArrival` construction sites (most of which — mark
//! seen, move category, rethread, migrate — have no verdict to offer)
//! would couple unrelated call paths to this feature.
//!
//! The caller decides *when* to write: only for inbound messages. A
//! user's own reply must not restate the thread's importance, mirroring
//! the display-field rule in `record_message_arrival` (2026-07-18).

use std::io;

use super::KevyMailboxStore;
use super::keys;

impl KevyMailboxStore {
    /// Store the importance verdict on a thread row.
    ///
    /// `level` is the stable lowercase token (`critical` / `important` /
    /// `normal` / `low` / `noise`) and `score` the raw numeric verdict.
    /// An empty `level` is a no-op: callers that could not compute a
    /// verdict must leave whatever the row already holds rather than
    /// blanking it.
    pub fn set_thread_importance(
        &self,
        thread_id: &str,
        level: &str,
        score: f64,
    ) -> io::Result<()> {
        if level.is_empty() {
            return Ok(());
        }
        let key = keys::thread(thread_id);
        let score_s = score.to_string();
        self.store().hset(
            key.as_bytes(),
            &[
                (b"importance_level" as &[u8], level.as_bytes()),
                (b"importance_score", score_s.as_bytes()),
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = Arc::new(Store::open(Config::default()).expect("open in-memory kevy"));
        KevyMailboxStore::new(s)
    }

    #[test]
    fn writes_and_reads_back() {
        let s = store();
        s.set_thread_importance("t1", "important", 0.65).unwrap();
        let row = s.get_thread("t1").unwrap().expect("row exists");
        assert_eq!(row.importance_level, "important");
        assert!((row.importance_score - 0.65).abs() < 1e-9);
    }

    #[test]
    fn empty_level_leaves_existing_verdict_alone() {
        let s = store();
        s.set_thread_importance("t1", "critical", 0.9).unwrap();
        // A caller with nothing to say must not blank the row.
        s.set_thread_importance("t1", "", 0.0).unwrap();
        let row = s.get_thread("t1").unwrap().expect("row exists");
        assert_eq!(row.importance_level, "critical");
        assert!((row.importance_score - 0.9).abs() < 1e-9);
    }

    #[test]
    fn later_verdict_overwrites() {
        let s = store();
        s.set_thread_importance("t1", "low", 0.1).unwrap();
        s.set_thread_importance("t1", "important", 0.7).unwrap();
        let row = s.get_thread("t1").unwrap().expect("row exists");
        assert_eq!(row.importance_level, "important");
    }
}
