//! What one reader's store says about each correspondent: when each
//! sender first wrote to them, when they first wrote to each address, and
//! the newest inbound message of each conversation. The backfill decides
//! first contact for mail delivered before sections recorded it from this,
//! where delivery read the contact record.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use mailrs_core_sidestate::families::contacts;
use mailrs_core_sidestate::families::correspondent::first_address;

use crate::FastcoreState;

/// How recent mail must be for the backfill to call it a first contact: a
/// stranger's letter from long ago is no longer a new sender.
pub(crate) const FIRST_CONTACT_WINDOW_SECS: i64 = 30 * 86_400;

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
    /// A message stored before this time is not a first contact.
    pub(crate) since: i64,
}

impl History {
    /// Every message of `user`'s in the store, read once, from its file
    /// as the section reads it (`From:`, `To:`, `Cc:` and the time it is
    /// stored under), so the history and a refold agree on who sent each
    /// message and when. A message whose file is gone counts from its
    /// stored row: its sender field is not always `From:`, but the count
    /// still says the store holds it. Only mail stored at `since` or later
    /// can be a first contact.
    pub(crate) fn build(
        state: &FastcoreState,
        user: &str,
        on_disk: &HashMap<String, PathBuf>,
        since: i64,
    ) -> std::io::Result<Self> {
        let mut h = History {
            user: user.to_string(),
            since,
            ..History::default()
        };
        for tid in state.mailbox.all_thread_ids_for_user(user)? {
            for (mid, at) in state.mailbox.user_thread_messages_dated(user, &tid)? {
                let blob_ref = state
                    .mailbox
                    .user_message_facts(user, &mid)?
                    .map(|c| c.blob_ref)
                    .unwrap_or_default();
                let read = match file_head(on_disk, &blob_ref) {
                    Some(head) => Some(from_file(&head, &blob_ref)),
                    None => state
                        .mailbox
                        .get_message(&mid)?
                        .and_then(|b| serde_json::from_slice::<Wire>(&b).ok())
                        .map(|w| (at, w.sender, w.recipients)),
                };
                let Some((at, from, recipients)) = read else {
                    continue;
                };
                match mailrs_mailbox::sent_copy::sent_by(&from, &blob_ref, user) {
                    true => h.wrote(&tid, at, &recipients),
                    false => h.received(&tid, at, mid, first_address(&from)),
                }
            }
        }
        Ok(h)
    }

    fn wrote(&mut self, tid: &str, at: i64, recipients: &str) {
        self.wrote_in.insert(tid.to_string());
        for addr in mailrs_rfc5322::split_list(recipients).map(mailrs_rfc5322::addr_spec) {
            let addr = addr.trim().to_ascii_lowercase();
            if addr.is_empty() || addr == self.user.to_ascii_lowercase() {
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

    /// Whether `mid` from `sender` was a first contact: stored at
    /// [`Self::since`] or later, the first message
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
        if *at < self.since || first != mid || self.first_out.get(sender).is_some_and(|t| t < at) {
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

/// The first 16 KiB of the message's file, `None` when it is not on disk.
fn file_head(on_disk: &HashMap<String, PathBuf>, blob_ref: &str) -> Option<Vec<u8>> {
    let mut raw = std::fs::read(on_disk.get(crate::maildir_scan::base_id(blob_ref))?).ok()?;
    raw.truncate(16 * 1024);
    Some(raw)
}

/// The stored time, `From:`, and `To:` with `Cc:`, as the section reads
/// a file.
fn from_file(head: &[u8], blob_ref: &str) -> (i64, String, String) {
    let (_, _, _, _, date, from, to) = mailrs_intake::headers::extract_headers(head);
    let cc = mailrs_rfc5322::Message::new(head)
        .header_str("Cc")
        .unwrap_or_default()
        .to_string();
    let at = mailrs_intake::headers::stored_date(date, blob_ref);
    (at, from, format!("{to},{cc}"))
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

    #[test]
    fn a_first_letter_from_before_the_window_is_not_a_first_contact() {
        let mut h = history();
        h.since = 15;
        let mut c = kevy_client::Connection::connect("mem://sender-history-window").unwrap();
        assert!(!h.first_contact("ann@y.test", "a1", Some(&mut c)), "at 10");
        assert!(!h.new_in("t1", Some(&mut c)));
        assert!(h.first_contact("cy@y.test", "c1", Some(&mut c)), "at 40");
        h.since = 41;
        assert!(!h.new_in("t5", Some(&mut c)));
    }
}
