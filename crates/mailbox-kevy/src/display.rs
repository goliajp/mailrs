//! What a conversation row says, for one reader: the `subject` and
//! `latest_preview` on their membership row, which the list renders and the
//! conversation search reads.
//!
//! They describe the messages this reader holds, the way the SQL core
//! aggregates them: the subject of the newest message they did not send
//! (the first of their own when they sent every one), and the preview of
//! the newest message with one, inbound before their own. The shared thread
//! hash keeps a subject and a preview too, from whichever member's mail
//! arrived last; the membership row used to be copied from it, so a member
//! could read the subject and opening line of a message that was never
//! delivered to them. `thread_user_pairs` no longer emits either field, and
//! [`write_display`] is the only thing that writes them.
//!
//! Two ways in, as for the participant list (`participants.rs`):
//! - an arrival describes its own message
//!   ([`KevyMailboxStore::add_display`]);
//! - anything that takes a message out of the reader's conversation or puts
//!   one in without an arrival recounts from the reader's own messages
//!   ([`KevyMailboxStore::recount_display`]).
//!
//! A message's preview is not stored with it; it is recomputed from the
//! body text the search index keeps (`mailrs:msgtext:<mid>`) with the
//! function intake uses. A recount keeps the stored preview when the reader
//! holds every message of the conversation, so it cannot have come from
//! anybody else's; otherwise it is the reader's own line, or empty when
//! none of their messages has an indexed body.
//!
//! [`write_display`]: KevyMailboxStore::write_display

use std::io;

use super::KevyMailboxStore;
use super::keys;

const SUBJECT: &[u8] = b"subject";
const PREVIEW: &[u8] = b"latest_preview";
/// The width intake cuts a preview to.
const PREVIEW_CHARS: usize = 120;

/// One of the reader's messages, as much of it as the row needs.
struct Held {
    mid: String,
    subject: String,
    date: i64,
    own: bool,
}

#[derive(serde::Deserialize)]
struct Wire {
    #[serde(default)]
    subject: String,
    #[serde(default)]
    sender: String,
    #[serde(default)]
    internal_date: i64,
}

/// What a recount found, for the correction route.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DisplayRecount {
    /// The row differs from the reader's messages (and was written unless
    /// a dry run).
    pub differs: bool,
    /// The stored subject is no message of the reader's, and is the
    /// subject of a message in the conversation they do not hold.
    pub foreign_subject: bool,
    /// The same for the preview, where the other messages' previews can be
    /// recomputed.
    pub foreign_preview: bool,
    /// What the reader's own messages say.
    pub subject: String,
    pub preview: String,
    /// How the row differs: its subject, and its preview from empty, to
    /// empty, or from one line to another.
    pub subject_differs: bool,
    pub preview_filled: bool,
    pub preview_blanked: bool,
    pub preview_replaced: bool,
}

impl KevyMailboxStore {
    fn held_messages(&self, user: &str, thread_id: &str) -> io::Result<Vec<Held>> {
        let mut out = Vec::new();
        for mid in self.user_thread_message_ids(user, thread_id)? {
            let Some(blob) = self.get_message(&mid)? else {
                continue;
            };
            let Ok(w) = serde_json::from_slice::<Wire>(&blob) else {
                continue;
            };
            out.push(Held {
                own: crate::senders_csv_contains_user(&w.sender, user),
                // Messages stored before intake decoded headers carry the
                // encoded words; the list shows text, as arrivals write it.
                subject: mailrs_rfc2047::decode(w.subject.as_bytes()).into_owned(),
                date: w.internal_date,
                mid,
            });
        }
        Ok(out)
    }

    /// The opening line of `mid`'s body, as intake cuts it, or empty when
    /// its body was never indexed.
    pub(crate) fn message_preview(&self, mid: &str) -> io::Result<String> {
        let body = self
            .store()
            .hget(keys::message_text(mid).as_bytes(), keys::MESSAGE_TEXT_FIELD)?;
        Ok(body
            .map(|b| mailrs_clean::preview_line(&String::from_utf8_lossy(&b), PREVIEW_CHARS))
            .unwrap_or_default())
    }

    /// Subject and preview from `user`'s own messages in `thread_id`, or
    /// `None` when they hold none. The preview is `None` when none of their
    /// messages has an indexed body.
    fn display_from(&self, held: &[Held]) -> io::Result<Option<(String, Option<String>)>> {
        if held.is_empty() {
            return Ok(None);
        }
        let newest_inbound = held.iter().filter(|h| !h.own).max_by_key(|h| h.date);
        let subject = match newest_inbound {
            Some(h) => h.subject.clone(),
            None => held
                .iter()
                .min_by_key(|h| h.date)
                .map(|h| h.subject.clone())
                .unwrap_or_default(),
        };
        // inbound first, then the reader's own; newest first within each
        let mut order: Vec<&Held> = held.iter().collect();
        order.sort_by_key(|h| (h.own, std::cmp::Reverse(h.date)));
        for h in order {
            let p = self.message_preview(&h.mid)?;
            if !p.is_empty() {
                return Ok(Some((subject, Some(p))));
            }
        }
        Ok(Some((subject, None)))
    }

    /// `user`'s stored subject and preview, or `None` when they have no row
    /// or the row has no subject yet.
    pub fn stored_display(
        &self,
        user: &str,
        thread_id: &str,
    ) -> io::Result<Option<(String, String)>> {
        let key = keys::thread_user(user, thread_id);
        let store = self.store();
        if !store.hexists(key.as_bytes(), b"tid")? || !store.hexists(key.as_bytes(), SUBJECT)? {
            return Ok(None);
        }
        let text = |f: &[u8]| -> io::Result<String> {
            Ok(store
                .hget(key.as_bytes(), f)?
                .map(|v| String::from_utf8_lossy(&v).into_owned())
                .unwrap_or_default())
        };
        Ok(Some((text(SUBJECT)?, text(PREVIEW)?)))
    }

    /// Put `user`'s subject and preview for `thread_id` back to what their
    /// own messages say, and say whether the row differed and whether what
    /// it showed came from somebody else's mail. A row with no messages of
    /// theirs is left alone.
    pub fn recount_display(
        &self,
        user: &str,
        thread_id: &str,
        dry_run: bool,
    ) -> io::Result<DisplayRecount> {
        let held = self.held_messages(user, thread_id)?;
        let Some((subject, computed)) = self.display_from(&held)? else {
            return Ok(DisplayRecount::default());
        };
        let stored = self.stored_display(user, thread_id)?;
        let (have_subject, have_preview) = stored.clone().unwrap_or_default();
        let mine: std::collections::HashSet<&str> = held.iter().map(|h| h.mid.as_str()).collect();
        let others: Vec<String> = self
            .store()
            .zrange(keys::thread_messages(thread_id).as_bytes(), 0, -1)?
            .into_iter()
            .filter_map(|(m, _)| String::from_utf8(m).ok())
            .filter(|m| !mine.contains(m.as_str()))
            .collect();
        // A conversation holding nobody else's message cannot be showing
        // anybody else's line, so a preview it has stays: recomputed from
        // the indexed body, the line differs from the stored one on about a
        // quarter of production's single-reader rows (written by older
        // preview code), and none of those differences is a leak. With
        // somebody else's message in it, the line is the reader's own or
        // nothing.
        let preview = match (others.is_empty(), computed) {
            (true, _) if !have_preview.is_empty() => have_preview.clone(),
            (_, Some(p)) => p,
            (_, None) => String::new(),
        };
        let mut r = DisplayRecount {
            differs: stored.is_none() || have_subject != subject || have_preview != preview,
            subject: subject.clone(),
            preview: preview.clone(),
            subject_differs: have_subject != subject,
            preview_filled: have_preview.is_empty() && !preview.is_empty(),
            preview_blanked: !have_preview.is_empty() && preview.is_empty(),
            preview_replaced: !have_preview.is_empty()
                && !preview.is_empty()
                && have_preview != preview,
            ..Default::default()
        };
        if r.differs && stored.is_some() {
            let own_subjects: std::collections::HashSet<&str> =
                held.iter().map(|h| h.subject.as_str()).collect();
            for mid in &others {
                let Some(blob) = self.get_message(mid)? else {
                    continue;
                };
                let Ok(w) = serde_json::from_slice::<Wire>(&blob) else {
                    continue;
                };
                let theirs = mailrs_rfc2047::decode(w.subject.as_bytes());
                if theirs == have_subject && !own_subjects.contains(have_subject.as_str()) {
                    r.foreign_subject = true;
                }
                if !have_preview.is_empty()
                    && have_preview != preview
                    && self.message_preview(mid)? == have_preview
                {
                    r.foreign_preview = true;
                }
            }
        }
        if r.differs && !dry_run {
            self.write_display(user, thread_id, &subject, &preview)?;
        }
        Ok(r)
    }

    /// An arriving message's subject and preview onto `user`'s row: theirs
    /// from somebody else always, their own only where the row has nothing
    /// yet. An arrival with no preview in hand keeps the row's.
    pub(crate) fn add_display(
        &self,
        user: &str,
        thread_id: &str,
        subject: &str,
        preview: &str,
        is_own: bool,
    ) -> io::Result<bool> {
        let (have_subject, have_preview) =
            self.stored_display(user, thread_id)?.unwrap_or_default();
        let subject = match is_own && !have_subject.is_empty() {
            true => have_subject.as_str(),
            false => subject,
        };
        let preview = match preview.is_empty() || (is_own && !have_preview.is_empty()) {
            true => have_preview.as_str(),
            false => preview,
        };
        self.write_display(user, thread_id, subject, preview)
    }

    /// The one writer of the two fields. Only on an existing row, so a
    /// subject cannot conjure membership, and only when they change.
    /// Returns whether it wrote.
    fn write_display(
        &self,
        user: &str,
        thread_id: &str,
        subject: &str,
        preview: &str,
    ) -> io::Result<bool> {
        let key = keys::thread_user(user, thread_id);
        let wrote = self.store().atomic(|ctx| {
            if !ctx.hexists(key.as_bytes(), b"tid")? {
                return Ok(false);
            }
            let same = ctx.hget(key.as_bytes(), SUBJECT)?.as_deref() == Some(subject.as_bytes())
                && ctx.hget(key.as_bytes(), PREVIEW)?.as_deref() == Some(preview.as_bytes());
            if same {
                return Ok(false);
            }
            ctx.hset(
                key.as_bytes(),
                &[(SUBJECT, subject.as_bytes()), (PREVIEW, preview.as_bytes())],
            )?;
            Ok(true)
        })?;
        Ok(wrote)
    }

    /// A row written from a caller's [`crate::ThreadRow`]: the reader's own
    /// messages decide when they hold any, and the caller's values stand
    /// only for a row with none (an import that writes the row before its
    /// messages, a fixture).
    pub(crate) fn display_after_upsert(
        &self,
        user: &str,
        thread_id: &str,
        subject: &str,
        preview: &str,
    ) -> io::Result<()> {
        if self.user_thread_message_ids(user, thread_id)?.is_empty() {
            self.write_display(user, thread_id, subject, preview)?;
            return Ok(());
        }
        self.recount_display(user, thread_id, false)?;
        Ok(())
    }

    /// A row derived from the shared hash gets its first subject and
    /// preview from the reader's own messages, never from the hash.
    pub(crate) fn seed_display(&self, user: &str, thread_id: &str) -> io::Result<()> {
        let key = keys::thread_user(user, thread_id);
        if self.store().hexists(key.as_bytes(), SUBJECT)? {
            return Ok(());
        }
        self.recount_display(user, thread_id, false)?;
        Ok(())
    }
}
