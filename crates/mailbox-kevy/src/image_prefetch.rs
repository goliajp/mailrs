//! The queue of remote images to fetch after delivery.

use std::io;

use crate::KevyMailboxStore;

fn member(user: &str, message_id: &str, url: &str) -> String {
    format!("{user}\t{message_id}\t{url}")
}

impl KevyMailboxStore {
    /// Queue `urls`, found in `user`'s message `message_id`, for fetching,
    /// in `set` ([`crate::keys::IMAGE_PREFETCH_PENDING`] or
    /// [`crate::keys::IMAGE_PREFETCH_ASKED`]).
    pub fn queue_image_prefetch(
        &self,
        set: &[u8],
        user: &str,
        message_id: &str,
        urls: &[String],
    ) -> io::Result<()> {
        if urls.is_empty() {
            return Ok(());
        }
        let members: Vec<String> = urls.iter().map(|u| member(user, message_id, u)).collect();
        let refs: Vec<&[u8]> = members.iter().map(|m| m.as_bytes()).collect();
        self.store().sadd(set, &refs)?;
        Ok(())
    }

    /// Up to `limit` queued `(user, message id, url)`, left in the queue.
    pub fn pending_image_prefetches(
        &self,
        set: &[u8],
        limit: usize,
    ) -> io::Result<Vec<(String, String, String)>> {
        let raw = self.store().srandmember(set, limit)?;
        Ok(raw
            .into_iter()
            .filter_map(|m| {
                let m = String::from_utf8(m).ok()?;
                let mut parts = m.splitn(3, '\t');
                Some((
                    parts.next()?.to_string(),
                    parts.next()?.to_string(),
                    parts.next()?.to_string(),
                ))
            })
            .collect())
    }

    /// Take one fetched or given-up image off the queue.
    pub fn done_image_prefetch(
        &self,
        set: &[u8],
        user: &str,
        message_id: &str,
        url: &str,
    ) -> io::Result<()> {
        let m = member(user, message_id, url);
        self.store().srem(set, &[m.as_bytes()])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kevy_embedded::{Config, Store};

    use crate::KevyMailboxStore;
    use crate::keys::{IMAGE_PREFETCH_ASKED as ASKED, IMAGE_PREFETCH_PENDING as PENDING};

    #[test]
    fn queued_images_come_back_once_and_leave() {
        let s = KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()));
        s.queue_image_prefetch(
            PENDING,
            "a@x.jp",
            "<m1@x>",
            &["https://e.example/a.png".into()],
        )
        .unwrap();
        assert!(s.pending_image_prefetches(ASKED, 10).unwrap().is_empty());
        let got = s.pending_image_prefetches(PENDING, 10).unwrap();
        assert_eq!(
            got,
            vec![(
                "a@x.jp".into(),
                "<m1@x>".into(),
                "https://e.example/a.png".into()
            )]
        );
        s.done_image_prefetch(PENDING, "a@x.jp", "<m1@x>", "https://e.example/a.png")
            .unwrap();
        assert!(s.pending_image_prefetches(PENDING, 10).unwrap().is_empty());
    }
}
