//! What one reader's store says about each correspondent: when each
//! sender first wrote to them, when they first wrote to each address, and
//! the newest inbound message of each conversation. The backfill decides
//! first contact for mail delivered before sections recorded it from this,
//! where delivery read the contact record.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use mailrs_core_sidestate::families::contacts;
use mailrs_core_sidestate::families::correspondent::{all_addresses, first_address};

use crate::FastcoreState;

#[derive(serde::Deserialize)]
struct Wire {
    #[serde(default)]
    sender: String,
    #[serde(default)]
    recipients: String,
}

#[derive(Default)]
pub(crate) struct History {
    user: String,
    /// Sender to the time and id of the first message they sent the reader.
    first_in: HashMap<String, (i64, String)>,
    received: HashMap<String, u64>,
    /// Address to when the reader first wrote to it, in `To:` or `Cc:`.
    first_out: HashMap<String, i64>,
    sent: HashMap<String, u64>,
    /// Thread to its newest message from someone else: time, id, sender.
    latest_in: HashMap<String, (i64, String, String)>,
    wrote_in: HashSet<String>,
}

impl History {
    /// Every message of `user`'s in the store, read once. The reader's own
    /// recipients come from the file (`Cc:` is not on the stored row), or
    /// the stored `To:` when the file is gone.
    pub(crate) fn build(
        state: &FastcoreState,
        user: &str,
        on_disk: &HashMap<String, PathBuf>,
    ) -> std::io::Result<Self> {
        let mut h = History {
            user: user.to_string(),
            ..History::default()
        };
        for tid in state.mailbox.all_thread_ids_for_user(user)? {
            for (mid, at) in state.mailbox.user_thread_messages_dated(user, &tid)? {
                let blob_ref = state
                    .mailbox
                    .user_message_facts(user, &mid)?
                    .map(|c| c.blob_ref)
                    .unwrap_or_default();
                let Some(wire) = state
                    .mailbox
                    .get_message(&mid)?
                    .and_then(|b| serde_json::from_slice::<Wire>(&b).ok())
                else {
                    continue;
                };
                match mailrs_mailbox::sent_copy::sent_by(&wire.sender, &blob_ref, user) {
                    true => {
                        let to = recipients(on_disk, &blob_ref).unwrap_or(wire.recipients);
                        h.wrote(&tid, at, &to);
                    }
                    false => h.received(&tid, at, mid, first_address(&wire.sender)),
                }
            }
        }
        Ok(h)
    }

    fn wrote(&mut self, tid: &str, at: i64, recipients: &str) {
        self.wrote_in.insert(tid.to_string());
        for addr in all_addresses(recipients) {
            if addr.eq_ignore_ascii_case(&self.user) {
                continue;
            }
            *self.sent.entry(addr.clone()).or_default() += 1;
            let first = self.first_out.entry(addr).or_insert(at);
            *first = (*first).min(at);
        }
    }

    fn received(&mut self, tid: &str, at: i64, mid: String, sender: String) {
        if sender.is_empty() {
            return;
        }
        *self.received.entry(sender.clone()).or_default() += 1;
        let first = self
            .first_in
            .entry(sender.clone())
            .or_insert((at, mid.clone()));
        if (at, &mid) < (first.0, &first.1) {
            *first = (at, mid.clone());
        }
        // a later or equal time replaces, as the section's fold does
        let newest = self.latest_in.get(tid).is_none_or(|(t, ..)| at >= *t);
        if newest {
            self.latest_in.insert(tid.to_string(), (at, mid, sender));
        }
    }

    /// Whether `mid` from `sender` was a first contact: the first message
    /// the sender ever sent the reader, the reader had written to them
    /// nothing before it, they are not accepted, and the contact counts
    /// show no mail between them the store no longer has. Without the
    /// network store there is no evidence either way, and it is not one.
    pub(crate) fn first_contact(
        &self,
        sender: &str,
        mid: &str,
        net: Option<&mut kevy_client::Connection>,
    ) -> bool {
        let Some((at, first)) = self.first_in.get(sender) else {
            return false;
        };
        if first != mid || self.first_out.get(sender).is_some_and(|t| t < at) {
            return false;
        }
        let Some(conn) = net else {
            return false;
        };
        if contacts::accepted(conn, &self.user, sender) {
            return false;
        }
        let (sent, received) = contacts::counts(conn, &self.user, sender);
        let held = |m: &HashMap<String, u64>| m.get(sender).copied().unwrap_or(0);
        u64::from(sent) <= held(&self.sent) && u64::from(received) <= held(&self.received)
    }

    /// Whether the thread's newest inbound message was a first contact and
    /// the reader has not written in it: what New holds.
    pub(crate) fn new_in(&self, tid: &str, net: Option<&mut kevy_client::Connection>) -> bool {
        if self.wrote_in.contains(tid) {
            return false;
        }
        self.latest_in
            .get(tid)
            .is_some_and(|(_, mid, sender)| self.first_contact(sender, mid, net))
    }
}

/// `To:` and `Cc:` of the file, joined; `None` when it is not on disk.
fn recipients(on_disk: &HashMap<String, PathBuf>, blob_ref: &str) -> Option<String> {
    let raw = std::fs::read(on_disk.get(crate::maildir_scan::base_id(blob_ref))?).ok()?;
    let head = &raw[..raw.len().min(16 * 1024)];
    let message = mailrs_rfc5322::Message::new(head);
    let to = message.header_str("To").unwrap_or_default();
    let cc = message.header_str("Cc").unwrap_or_default();
    Some(format!("{to},{cc}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: &str = "me@x.test";

    fn history() -> History {
        let mut h = History {
            user: ME.into(),
            ..History::default()
        };
        h.received("t1", 10, "a1".into(), "ann@y.test".into());
        h.received("t2", 30, "a2".into(), "ann@y.test".into());
        h.wrote("t3", 5, "Bo <bo@y.test>, me@x.test");
        h.received("t4", 20, "b1".into(), "bo@y.test".into());
        h.received("t5", 40, "c1".into(), "cy@y.test".into());
        h.wrote("t6", 50, "cy@y.test");
        h
    }

    fn counts(c: &mut kevy_client::Connection, who: &str, sent: &[u8], received: &[u8]) {
        c.hset(
            contacts::contact_key(ME, who).as_bytes(),
            &[
                (b"sent_count" as &[u8], sent),
                (b"received_count" as &[u8], received),
            ],
        )
        .unwrap();
    }

    #[test]
    fn only_a_senders_first_letter_before_any_of_mine_is_a_first_contact() {
        let h = history();
        let mut c = kevy_client::Connection::connect("mem://sender-history-first").unwrap();
        assert!(h.first_contact("ann@y.test", "a1", Some(&mut c)));
        assert!(!h.first_contact("ann@y.test", "a2", Some(&mut c)), "second");
        assert!(
            !h.first_contact("bo@y.test", "b1", Some(&mut c)),
            "I wrote first"
        );
        assert!(
            h.first_contact("cy@y.test", "c1", Some(&mut c)),
            "I wrote after"
        );
        assert!(
            !h.first_contact("eve@z.test", "e1", Some(&mut c)),
            "unknown"
        );
        assert!(!h.first_contact("ann@y.test", "a1", None), "no evidence");
    }

    #[test]
    fn counts_the_store_cannot_account_for_or_an_acceptance_rule_it_out() {
        let h = history();
        let mut c = kevy_client::Connection::connect("mem://sender-history-counts").unwrap();
        counts(&mut c, "ann@y.test", b"0", b"2");
        assert!(h.first_contact("ann@y.test", "a1", Some(&mut c)));
        counts(&mut c, "ann@y.test", b"0", b"3");
        assert!(!h.first_contact("ann@y.test", "a1", Some(&mut c)), "lost");
        counts(&mut c, "cy@y.test", b"2", b"1");
        assert!(
            !h.first_contact("cy@y.test", "c1", Some(&mut c)),
            "sent lost"
        );
        contacts::accept(&mut c, ME, "eve@z.test", 1).unwrap();
        let mut h = h;
        h.received("t7", 60, "e1".into(), "eve@z.test".into());
        assert!(
            !h.first_contact("eve@z.test", "e1", Some(&mut c)),
            "accepted"
        );
    }

    #[test]
    fn a_thread_is_new_by_its_newest_inbound_letter_unless_i_wrote_in_it() {
        let mut h = history();
        let mut c = kevy_client::Connection::connect("mem://sender-history-new").unwrap();
        assert!(h.new_in("t1", Some(&mut c)));
        assert!(!h.new_in("t2", Some(&mut c)));
        assert!(!h.new_in("t3", Some(&mut c)), "only mine");
        h.received("t1", 70, "d1".into(), "dee@y.test".into());
        assert!(
            h.new_in("t1", Some(&mut c)),
            "dee's first is now the newest"
        );
        h.received("t1", 80, "a3".into(), "ann@y.test".into());
        assert!(
            !h.new_in("t1", Some(&mut c)),
            "ann's third is now the newest"
        );
        h.wrote("t5", 90, "cy@y.test");
        assert!(!h.new_in("t5", Some(&mut c)), "I wrote in it");
    }
}
