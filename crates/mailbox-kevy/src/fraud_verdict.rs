//! The stored fraud finding for one message.
//!
//! Written once, at receive time, by the only process that saw the
//! SMTP transaction; read by the review screen, which has to show what
//! was decided **then** rather than what today's rules would decide.
//! Nothing recomputes it in place.
//!
//! Opaque bytes here on purpose. The shape belongs to
//! `mailrs_inbound::FraudVerdict`, which owns both the assembly and
//! the round-trip test; a second definition of it in this crate would
//! be a second definition to drift.

use std::io;

use super::KevyMailboxStore;
use super::keys;

impl KevyMailboxStore {
    /// Record what was found about `message_id`.
    ///
    /// Overwrites: a message is received once, and the only writer is
    /// the receive path. A re-scan that wanted to replace a verdict
    /// would be recomputing it, which is what this deliberately does
    /// not do.
    pub fn set_fraud_verdict(&self, message_id: &str, verdict_json: &str) -> io::Result<()> {
        self.store()
            .set(
                keys::fraud_verdict(message_id).as_bytes(),
                verdict_json.as_bytes(),
            )
            .map_err(io::Error::from)?;
        Ok(())
    }

    /// What was found about `message_id`, if anything was.
    ///
    /// `None` is the ordinary answer and means *nobody suspected this
    /// message* — not *it was examined and cleared*. The two are
    /// different facts and the screen must not render them alike.
    pub fn fraud_verdict(&self, message_id: &str) -> io::Result<Option<String>> {
        let raw = self
            .store()
            .get(keys::fraud_verdict(message_id).as_bytes())
            .map_err(io::Error::from)?;
        Ok(raw.map(|b| String::from_utf8_lossy(&b).into_owned()))
    }
}

#[cfg(test)]
mod tests {
    use crate::KevyMailboxStore;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ))
    }

    #[test]
    fn a_verdict_comes_back_as_it_went_in() {
        let st = store();
        let json = r#"{"rules_version":"2026-08-28.1","score":9.5}"#;
        st.set_fraud_verdict("<m1@x.com>", json).unwrap();
        assert_eq!(
            st.fraud_verdict("<m1@x.com>").unwrap().as_deref(),
            Some(json)
        );
    }

    /// Nothing found is `None`, and `None` has to be distinguishable
    /// from an empty verdict — "nobody suspected this" and "it was
    /// examined and cleared" are different facts.
    #[test]
    fn an_unexamined_message_has_no_verdict() {
        assert_eq!(store().fraud_verdict("<never@x.com>").unwrap(), None);
    }

    /// One message, one verdict. Two readers of the same message do
    /// not each get their own — who is *holding* it is the per-user
    /// `quarantined` column's question.
    #[test]
    fn it_is_keyed_by_message_and_not_by_reader() {
        let st = store();
        st.set_fraud_verdict("<m1@x.com>", r#"{"score":1}"#)
            .unwrap();
        st.set_fraud_verdict("<m2@x.com>", r#"{"score":2}"#)
            .unwrap();
        assert_eq!(
            st.fraud_verdict("<m1@x.com>").unwrap().as_deref(),
            Some(r#"{"score":1}"#)
        );
    }
}
