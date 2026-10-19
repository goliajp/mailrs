//! Text extracted from attachments: the queue, the stored rows, and search.
//!
//! Delivery only queues a message. A worker in fastcore reads the file,
//! extracts the text and calls [`KevyMailboxStore::store_attachment_texts`],
//! which writes one indexed row per attachment and a state hash saying the
//! message is done. The state is what keeps a message from being queued and
//! extracted twice, and what [`KevyMailboxStore::delete_thread`] reads to
//! find the rows to delete.

use std::io;

use crate::KevyMailboxStore;
use crate::keys;

/// What one extraction pass made of a message's attachments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachmentTextState {
    /// Extractor generation that wrote this. Rows from before the field
    /// existed read as 1: PDF text only, no OCR.
    pub version: u32,
    /// Attachments considered (PDFs and images, in MIME order). Rows are
    /// numbered by position among these.
    pub parts: u32,
    /// Of those, PDFs.
    pub pdfs: u32,
    /// Of those, images.
    pub images: u32,
    /// Attachments whose text was stored.
    pub stored: u32,
    /// Of the stored, how many were read by OCR.
    pub ocr: u32,
    /// Reading failed: unreadable file, a panic, or OCR gave up.
    pub failed: u32,
    /// Skipped without reading, over the size limit.
    pub too_large: u32,
    /// Images too small to carry text worth reading.
    pub too_small: u32,
    /// Stored, but cut at [`keys::ATTACHMENT_TEXT_CAP`].
    pub truncated: u32,
}

impl AttachmentTextState {
    fn fields(&self) -> [(&'static str, u32); 10] {
        [
            ("version", self.version),
            ("parts", self.parts),
            ("pdfs", self.pdfs),
            ("images", self.images),
            ("stored", self.stored),
            ("ocr", self.ocr),
            ("failed", self.failed),
            ("too_large", self.too_large),
            ("too_small", self.too_small),
            ("truncated", self.truncated),
        ]
    }

    fn set(&mut self, name: &str, v: u32) {
        let slot = match name {
            "version" => &mut self.version,
            "parts" => &mut self.parts,
            "pdfs" => &mut self.pdfs,
            "images" => &mut self.images,
            "stored" => &mut self.stored,
            "ocr" => &mut self.ocr,
            "failed" => &mut self.failed,
            "too_large" => &mut self.too_large,
            "too_small" => &mut self.too_small,
            "truncated" => &mut self.truncated,
            _ => return,
        };
        *slot = v;
    }
}

fn member(user: &str, message_id: &str) -> String {
    format!("{user}\t{message_id}")
}

impl KevyMailboxStore {
    /// Queue one recipient's copy of a message for extraction.
    pub fn queue_attachment_text(&self, user: &str, message_id: &str) -> io::Result<()> {
        let m = member(user, message_id);
        self.store()
            .sadd(keys::ATTACHMENT_TEXT_PENDING, &[m.as_bytes()])?;
        Ok(())
    }

    /// Up to `limit` queued `(user, message_id)` pairs, left in the queue.
    pub fn pending_attachment_texts(&self, limit: usize) -> io::Result<Vec<(String, String)>> {
        let raw = self
            .store()
            .srandmember(keys::ATTACHMENT_TEXT_PENDING, limit)?;
        Ok(raw
            .into_iter()
            .filter_map(|m| {
                let m = String::from_utf8(m).ok()?;
                let (user, mid) = m.split_once('\t')?;
                Some((user.to_string(), mid.to_string()))
            })
            .collect())
    }

    /// Take one pair off the queue.
    pub fn dequeue_attachment_text(&self, user: &str, message_id: &str) -> io::Result<()> {
        let m = member(user, message_id);
        self.store()
            .srem(keys::ATTACHMENT_TEXT_PENDING, &[m.as_bytes()])?;
        Ok(())
    }

    /// Whether `user`'s copy of the message is queued.
    pub fn attachment_text_queued(&self, user: &str, message_id: &str) -> io::Result<bool> {
        let m = member(user, message_id);
        Ok(self
            .store()
            .sismember(keys::ATTACHMENT_TEXT_PENDING, m.as_bytes())?)
    }

    /// Store the outcome of extracting a message's attachments.
    ///
    /// `texts` pairs each attachment's position among the message's PDFs
    /// with its text, already capped. Rows from an earlier pass are
    /// replaced, so storing twice leaves what the second pass found.
    pub fn store_attachment_texts(
        &self,
        message_id: &str,
        texts: &[(usize, String)],
        state: &AttachmentTextState,
    ) -> io::Result<()> {
        for key in self.attachment_text_keys(message_id)? {
            self.store().del(&[key.as_bytes()])?;
        }
        for (n, text) in texts {
            let text = keys::cap_text(text.trim(), keys::ATTACHMENT_TEXT_CAP);
            if text.is_empty() {
                continue;
            }
            self.store().hset(
                keys::attachment_text(message_id, *n).as_bytes(),
                &[(keys::ATTACHMENT_TEXT_FIELD, text.as_bytes())],
            )?;
        }
        let values: Vec<(&str, String)> = state
            .fields()
            .iter()
            .map(|(k, v)| (*k, v.to_string()))
            .collect();
        let pairs: Vec<(&[u8], &[u8])> = values
            .iter()
            .map(|(k, v)| (k.as_bytes(), v.as_bytes()))
            .collect();
        self.store()
            .hset(keys::attachment_text_state(message_id).as_bytes(), &pairs)?;
        Ok(())
    }

    /// The outcome stored for a message, or `None` if it was never processed.
    pub fn attachment_text_state(
        &self,
        message_id: &str,
    ) -> io::Result<Option<AttachmentTextState>> {
        let flat = self
            .store()
            .hgetall(keys::attachment_text_state(message_id).as_bytes())?;
        if flat.is_empty() {
            return Ok(None);
        }
        let mut out = AttachmentTextState::default();
        let has = |name: &str| flat.iter().any(|(f, _)| f.as_slice() == name.as_bytes());
        let (versioned, counted) = (has("version"), has("parts"));
        for (field, value) in &flat {
            let v: u32 = std::str::from_utf8(value)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            out.set(std::str::from_utf8(field).unwrap_or(""), v);
        }
        if !versioned {
            out.version = 1;
        }
        if !counted {
            out.parts = out.pdfs;
        }
        Ok(Some(out))
    }

    /// The stored text of each attachment, in attachment order.
    pub fn attachment_texts(&self, message_id: &str) -> io::Result<Vec<String>> {
        let mut out = Vec::new();
        for key in self.attachment_text_keys(message_id)? {
            if let Some(v) = self
                .store()
                .hget(key.as_bytes(), keys::ATTACHMENT_TEXT_FIELD)?
            {
                out.push(String::from_utf8_lossy(&v).into_owned());
            }
        }
        Ok(out)
    }

    /// Every key extraction wrote for a message: its rows and its state.
    pub(crate) fn attachment_text_keys(&self, message_id: &str) -> io::Result<Vec<String>> {
        let Some(state) = self.attachment_text_state(message_id)? else {
            return Ok(Vec::new());
        };
        let mut out: Vec<String> = (0..state.parts as usize)
            .map(|n| keys::attachment_text(message_id, n))
            .collect();
        out.push(keys::attachment_text_state(message_id));
        Ok(out)
    }

    /// Thread ids of the caller's conversations whose attachments match
    /// `query`, best first and de-duplicated. A row names a message, not a
    /// thread; the thread comes from the caller's own row for that message,
    /// as for body search.
    pub fn search_attachment_texts(
        &self,
        user: &str,
        query: &str,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        self.search_message_rows(
            keys::IDX_ATTACHMENT_TEXT,
            keys::ATTTEXT_PREFIX,
            true,
            user,
            query,
            limit,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        let s = KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()));
        s.ensure_admin_indexes();
        s
    }

    fn deliver(s: &KevyMailboxStore, user: &str, tid: &str, mid: &str) {
        s.record_message_arrival(&crate::MessageArrival {
            thread_id: tid,
            user,
            subject: "s",
            senders_csv: "a@x.com",
            latest_date: 1,
            latest_preview: "",
            category: "inbox",
            unread: true,
            is_own: false,
        })
        .unwrap();
        s.upsert_user_message(
            user,
            tid,
            mid,
            1,
            b"{}",
            &crate::UserMessageFacts {
                blob_ref: "f1",
                uid: 1,
                flags: 0,
                modseq: 0,
            },
        )
        .unwrap();
    }

    fn done(pdfs: u32) -> AttachmentTextState {
        AttachmentTextState {
            version: 2,
            parts: pdfs,
            pdfs,
            stored: pdfs,
            ..Default::default()
        }
    }

    #[test]
    fn a_word_in_an_attachment_finds_the_recipients_thread_only() {
        let s = store();
        deliver(&s, "bob@x.com", "t1", "m1");
        deliver(&s, "carol@x.com", "t9", "m9");
        s.store_attachment_texts("m1", &[(0, "invoice zebrafinch".into())], &done(1))
            .unwrap();
        assert_eq!(
            s.search_attachment_texts("bob@x.com", "zebrafinch", 10)
                .unwrap(),
            vec!["t1".to_string()]
        );
        assert!(
            s.search_attachment_texts("carol@x.com", "zebrafinch", 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn deleting_the_thread_deletes_the_rows_and_the_state() {
        let s = store();
        deliver(&s, "bob@x.com", "t1", "m1");
        s.store_attachment_texts("m1", &[(0, "first".into()), (1, "second".into())], &done(2))
            .unwrap();
        assert_eq!(s.attachment_texts("m1").unwrap().len(), 2);
        assert!(s.delete_thread("bob@x.com", "t1").unwrap().0);
        assert!(s.attachment_texts("m1").unwrap().is_empty());
        assert_eq!(s.attachment_text_state("m1").unwrap(), None);
        for n in 0..2 {
            let key = keys::attachment_text("m1", n);
            assert_eq!(s.store().exists(&[key.as_bytes()]).unwrap(), 0);
        }
    }

    #[test]
    fn the_queue_holds_pairs_until_each_is_taken_off() {
        let s = store();
        s.queue_attachment_text("bob@x.com", "m1").unwrap();
        s.queue_attachment_text("bob@x.com", "m1").unwrap();
        s.queue_attachment_text("carol@x.com", "m1").unwrap();
        let mut got = s.pending_attachment_texts(10).unwrap();
        got.sort();
        assert_eq!(
            got,
            vec![
                ("bob@x.com".to_string(), "m1".to_string()),
                ("carol@x.com".to_string(), "m1".to_string()),
            ]
        );
        s.dequeue_attachment_text("bob@x.com", "m1").unwrap();
        assert!(!s.attachment_text_queued("bob@x.com", "m1").unwrap());
        assert_eq!(s.pending_attachment_texts(10).unwrap().len(), 1);
    }

    /// A state written before OCR existed has neither `version` nor
    /// `parts`; it reads as version 1 with one part per PDF, so its rows
    /// can still be found and replaced.
    #[test]
    fn a_state_from_before_ocr_reads_as_version_one() {
        let s = store();
        s.store_attachment_texts("m1", &[(0, "a".into()), (1, "b".into())], &done(2))
            .unwrap();
        let key = keys::attachment_text_state("m1");
        s.store()
            .hdel(key.as_bytes(), &[b"version" as &[u8], b"parts"])
            .unwrap();
        let old = s.attachment_text_state("m1").unwrap().unwrap();
        assert_eq!((old.version, old.parts, old.pdfs), (1, 2, 2));
        let ocr = AttachmentTextState {
            version: 2,
            parts: 1,
            images: 1,
            stored: 1,
            ocr: 1,
            ..Default::default()
        };
        s.store_attachment_texts("m1", &[(0, "c".into())], &ocr)
            .unwrap();
        assert_eq!(s.attachment_texts("m1").unwrap(), vec!["c".to_string()]);
        assert_eq!(s.attachment_text_state("m1").unwrap(), Some(ocr));
    }

    #[test]
    fn stored_text_is_capped_on_a_char_boundary() {
        let s = store();
        let long = "日".repeat(keys::ATTACHMENT_TEXT_CAP);
        s.store_attachment_texts("m1", &[(0, long)], &done(1))
            .unwrap();
        let back = s.attachment_texts("m1").unwrap();
        assert!(back[0].len() <= keys::ATTACHMENT_TEXT_CAP);
        assert!(back[0].len() > keys::ATTACHMENT_TEXT_CAP - 3);
    }
}
