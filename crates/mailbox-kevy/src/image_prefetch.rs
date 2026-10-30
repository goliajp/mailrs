//! The queue of remote images to fetch after delivery.

use std::io;

use crate::KevyMailboxStore;
use crate::keys;

fn member(user: &str, message_id: &str, url: &str) -> String {
    format!("{user}\t{message_id}\t{url}")
}

impl KevyMailboxStore {
    /// Queue `urls`, found in `user`'s message `message_id`, for fetching.
    pub fn queue_image_prefetch(
        &self,
        user: &str,
        message_id: &str,
        urls: &[String],
    ) -> io::Result<()> {
        if urls.is_empty() {
            return Ok(());
        }
        let members: Vec<String> = urls.iter().map(|u| member(user, message_id, u)).collect();
        let refs: Vec<&[u8]> = members.iter().map(|m| m.as_bytes()).collect();
        self.store().sadd(keys::IMAGE_PREFETCH_PENDING, &refs)?;
        Ok(())
    }

    /// Up to `limit` queued `(user, message id, url)`, left in the queue.
    pub fn pending_image_prefetches(
        &self,
        limit: usize,
    ) -> io::Result<Vec<(String, String, String)>> {
        let raw = self
            .store()
            .srandmember(keys::IMAGE_PREFETCH_PENDING, limit)?;
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
    pub fn done_image_prefetch(&self, user: &str, message_id: &str, url: &str) -> io::Result<()> {
        let m = member(user, message_id, url);
        self.store()
            .srem(keys::IMAGE_PREFETCH_PENDING, &[m.as_bytes()])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kevy_embedded::{Config, Store};

    use crate::KevyMailboxStore;

    #[test]
    fn queued_images_come_back_once_and_leave() {
        let s = KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()));
        s.queue_image_prefetch("a@x.jp", "<m1@x>", &["https://e.example/a.png".into()])
            .unwrap();
        let got = s.pending_image_prefetches(10).unwrap();
        assert_eq!(
            got,
            vec![(
                "a@x.jp".into(),
                "<m1@x>".into(),
                "https://e.example/a.png".into()
            )]
        );
        s.done_image_prefetch("a@x.jp", "<m1@x>", "https://e.example/a.png")
            .unwrap();
        assert!(s.pending_image_prefetches(10).unwrap().is_empty());
    }
}
