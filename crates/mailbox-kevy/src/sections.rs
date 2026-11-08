//! The sectioned inbox on the membership row: the inputs
//! `mailrs_mailbox::section` decides from, kept by the writers that change
//! them, and the two declared columns it produces (`section`,
//! `awaiting_since`).
//!
//! The inputs ride on the row undeclared — nothing indexes them — and the
//! two outputs are written in the same atomic block that changed an input,
//! so a list never shows a section its inputs no longer support.

use std::io;

use mailrs_mailbox::section::{MessageFacts, Section};

use crate::KevyMailboxStore;
use crate::keys;

/// One message, as the section reads it.
#[derive(Debug, Clone, Copy)]
pub struct SectionArrival<'a> {
    pub user: &'a str,
    pub thread_id: &'a str,
    /// Epoch seconds the message sorts by.
    pub at: i64,
    pub is_own: bool,
    pub facts: MessageFacts,
    /// The sender's bare address, lowercased; what an acceptance matches.
    pub from_addr: &'a str,
}

/// The declared columns this module writes and nothing else does.
pub(crate) const COLUMNS: [&str; 3] = ["section", "awaiting_since", "sec_in_from"];

const OWN_AT: &[u8] = b"sec_own_at";
const OWN_TO_OTHERS: &[u8] = b"sec_own_to_others";
const PERSON_AT: &[u8] = b"sec_person_at";
const IN_AT: &[u8] = b"sec_in_at";
const IN_DIRECT: &[u8] = b"sec_in_direct";
const IN_PERSON: &[u8] = b"sec_in_person";
const IN_FIRST: &[u8] = b"sec_in_first";
const IN_FROM: &[u8] = b"sec_in_from";
const IN_ACCEPTED: &[u8] = b"sec_in_accepted";
const IN_INVITE: &[u8] = b"sec_in_invite";
const IN_MUTUAL: &[u8] = b"sec_in_mutual";

/// A refold's outcome: the row's section before and after, `""` for none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refold {
    pub before: String,
    pub after: String,
}

fn flag(on: bool) -> &'static [u8] {
    match on {
        true => b"1",
        false => b"0",
    }
}

impl KevyMailboxStore {
    /// Fold one message into `user`'s row for the thread and rewrite its
    /// section. An older message than the one already recorded on its side
    /// changes nothing, so delivering the same mail twice, or out of order,
    /// ends in the same row.
    pub fn record_section_arrival(&self, a: &SectionArrival<'_>) -> io::Result<()> {
        let key = keys::thread_user(a.user, a.thread_id);
        let at = a.at.to_string();
        self.store()
            .atomic(|ctx| {
                let mut stored = [0i64; 3];
                for (slot, field) in stored.iter_mut().zip([OWN_AT, IN_AT, PERSON_AT]) {
                    *slot = ctx
                        .hget(key.as_bytes(), field)?
                        .and_then(|v| String::from_utf8(v).ok())
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                }
                let [own_at, in_at, person_at] = stored;
                if a.is_own {
                    if a.at >= own_at {
                        ctx.hset(
                            key.as_bytes(),
                            &[
                                (OWN_AT, at.as_bytes()),
                                (OWN_TO_OTHERS, flag(a.facts.to_others)),
                            ],
                        )?;
                    }
                } else {
                    if a.at >= in_at {
                        ctx.hset(
                            key.as_bytes(),
                            &[
                                (IN_AT, at.as_bytes()),
                                (IN_DIRECT, flag(a.facts.direct)),
                                (IN_PERSON, flag(a.facts.from_person)),
                                (IN_FIRST, flag(a.facts.first_contact)),
                                (IN_FROM, a.from_addr.as_bytes()),
                                (IN_ACCEPTED, b"0"),
                                (IN_INVITE, flag(a.facts.invite)),
                                (IN_MUTUAL, flag(a.facts.mutual)),
                            ],
                        )?;
                    }
                    if a.facts.from_person && a.at >= person_at {
                        ctx.hset(key.as_bytes(), &[(PERSON_AT, at.as_bytes())])?;
                    }
                }
                Ok(())
            })
            .map_err(io::Error::from)?;
        self.rewrite_section(&key)
    }

    /// Decide the row's section again from `arrivals` alone: what is left
    /// of the conversation after a message went. A fold only moves
    /// forward, so the inputs restart from nothing.
    ///
    /// Whether the newest inbound message was a first contact, and whether
    /// its sender and the reader wrote to each other, were read at its
    /// delivery from a contact history that has moved on since; when that
    /// message is still the newest inbound one, its recorded answers (and
    /// an acceptance of its sender) stand. A row recorded before `mutual`
    /// existed has no answer to keep and takes the arrival's. With no
    /// arrivals the row is left as it was: there is nothing to decide from.
    pub fn refold_section(
        &self,
        user: &str,
        thread_id: &str,
        arrivals: &[SectionArrival<'_>],
    ) -> io::Result<()> {
        self.refold_section_report(user, thread_id, arrivals, true)
            .map(|_| ())
    }

    /// [`Self::refold_section`], saying what the section was and becomes;
    /// with `write` false nothing is written. `None` with no arrivals.
    pub fn refold_section_report(
        &self,
        user: &str,
        thread_id: &str,
        arrivals: &[SectionArrival<'_>],
        write: bool,
    ) -> io::Result<Option<Refold>> {
        if arrivals.is_empty() {
            return Ok(None);
        }
        let key = keys::thread_user(user, thread_id);
        let old: Fields = self.store().hgetall(key.as_bytes())?.into_iter().collect();
        let (latest, from) = refolded(&old, arrivals);
        let before = text(&old, b"section");
        let after = decided(&old, &latest).0.to_string();
        if write {
            let (own_at, person_at, in_at) = (
                latest.own_at.to_string(),
                latest.person_at.to_string(),
                latest.other_at.to_string(),
            );
            let o = latest.other;
            self.store()
                .atomic(|ctx| {
                    ctx.hset(
                        key.as_bytes(),
                        &[
                            (OWN_AT, own_at.as_bytes()),
                            (OWN_TO_OTHERS, flag(latest.own_to_others)),
                            (PERSON_AT, person_at.as_bytes()),
                            (IN_AT, in_at.as_bytes()),
                            (IN_DIRECT, flag(o.direct)),
                            (IN_PERSON, flag(o.from_person)),
                            (IN_FIRST, flag(o.first_contact)),
                            (IN_FROM, from.as_bytes()),
                            (IN_ACCEPTED, flag(latest.other_accepted)),
                            (IN_INVITE, flag(o.invite)),
                            (IN_MUTUAL, flag(o.mutual)),
                        ],
                    )?;
                    Ok(())
                })
                .map_err(io::Error::from)?;
            self.rewrite_section(&key)?;
        }
        Ok(Some(Refold { before, after }))
    }

    /// Whether `user`'s row for the thread records an inbound message but
    /// not the facts added after sections first shipped (`invite`,
    /// `mutual`), so that it reads them as false until refolded.
    pub fn section_facts_outdated(&self, user: &str, thread_id: &str) -> io::Result<bool> {
        let key = keys::thread_user(user, thread_id);
        let fields: Fields = self.store().hgetall(key.as_bytes())?.into_iter().collect();
        Ok(num(&fields, IN_AT) > 0 && !fields.contains_key(IN_MUTUAL))
    }

    /// The section `user`'s row for `thread_id` is in, `None` when no
    /// writer has reached the row since sections existed.
    pub fn section_of_thread(&self, user: &str, thread_id: &str) -> io::Result<Option<Section>> {
        let key = keys::thread_user(user, thread_id);
        Ok(self
            .store()
            .hget(key.as_bytes(), b"section")?
            .and_then(|v| String::from_utf8(v).ok())
            .and_then(|v| Section::named(&v)))
    }

    /// Whether `user` has written in the thread, as the section reads it:
    /// the row records a message of theirs.
    pub fn reader_wrote_in(&self, user: &str, thread_id: &str) -> io::Result<bool> {
        let key = keys::thread_user(user, thread_id);
        Ok(self
            .store()
            .hget(key.as_bytes(), OWN_AT)?
            .and_then(|v| String::from_utf8(v).ok())
            .and_then(|v| v.parse::<i64>().ok())
            .is_some_and(|at| at > 0))
    }

    /// The row's `section` (empty when undecided) and `awaiting_since`.
    pub fn section_fields(&self, user: &str, thread_id: &str) -> io::Result<(String, i64)> {
        let key = keys::thread_user(user, thread_id);
        let read = |f: &[u8]| -> io::Result<String> {
            Ok(self
                .store()
                .hget(key.as_bytes(), f)?
                .and_then(|v| String::from_utf8(v).ok())
                .unwrap_or_default())
        };
        let since = read(b"awaiting_since")?.parse().unwrap_or(0);
        Ok((read(b"section")?, since))
    }

    /// Accept `senders` (bare, lowercased): every conversation of `user`'s
    /// in the New section whose last message came from one of them is
    /// decided again. Returns how many moved. The acceptance itself is a
    /// fact the caller records on the contact; this only follows from it.
    pub fn accept_senders(&self, user: &str, senders: &[String]) -> io::Result<usize> {
        let tids = self.list_thread_ids_by_section_via_table(
            user,
            Section::New,
            crate::ArchiveScope::All,
            usize::MAX,
        )?;
        let mut moved = 0;
        for tid in tids {
            let key = keys::thread_user(user, &tid);
            let from = self
                .store()
                .hget(key.as_bytes(), IN_FROM)?
                .and_then(|v| String::from_utf8(v).ok())
                .unwrap_or_default();
            if !senders.iter().any(|s| s.eq_ignore_ascii_case(&from)) {
                continue;
            }
            self.store()
                .hset(key.as_bytes(), &[(IN_ACCEPTED, b"1" as &[u8])])?;
            self.rewrite_section(&key)?;
            moved += 1;
        }
        Ok(moved)
    }

    /// Write `section` and `awaiting_since` from the row's inputs, only
    /// when they differ: a backfill pass over correct rows writes nothing.
    ///
    /// A row no message has been recorded on stays undecided, and a
    /// conversation in Junk is in no section: the sectioned inbox is the
    /// mail that is not Junk.
    pub(crate) fn rewrite_section(&self, key: &str) -> io::Result<()> {
        let fields: Fields = self.store().hgetall(key.as_bytes())?.into_iter().collect();
        if !fields.contains_key(OWN_AT) && !fields.contains_key(IN_AT) {
            return Ok(());
        }
        let (section, since) = decided(&fields, &latest_from(&fields));
        let same = |f: &[u8], want: &str| fields.get(f).map(Vec::as_slice) == Some(want.as_bytes());
        if same(b"section", section) && same(b"awaiting_since", &since) {
            return Ok(());
        }
        self.store().atomic(|ctx| {
            crate::thread_row::hset_membership(
                ctx,
                key.as_bytes(),
                &[
                    (b"section" as &[u8], section.as_bytes()),
                    (b"awaiting_since", since.as_bytes()),
                ],
            )
        })?;
        Ok(())
    }
}

#[path = "sections_row.rs"]
mod row;
use row::{Fields, decided, latest_from, num, refolded, text};

#[cfg(test)]
#[path = "sections_tests.rs"]
mod tests;
