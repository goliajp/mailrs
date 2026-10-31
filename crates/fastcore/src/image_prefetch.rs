//! fastcore's side of the remote-image prefetch: the queue lives in its
//! embedded store. What is queued, the worker and the cache are shared
//! with the SQL core in `mailrs_intake::image_prefetch`.

use std::io;
use std::sync::Arc;

use mailrs_intake::image_prefetch::{self as shared, PrefetchQueue};

use crate::FastcoreState;

impl PrefetchQueue for FastcoreState {
    fn queue(&self, user: &str, message_id: &str, urls: &[String]) -> io::Result<()> {
        self.mailbox.queue_image_prefetch(user, message_id, urls)
    }
    fn pending(&self, limit: usize) -> io::Result<Vec<(String, String, String)>> {
        self.mailbox.pending_image_prefetches(limit)
    }
    fn done(&self, user: &str, message_id: &str, url: &str) -> io::Result<()> {
        self.mailbox.done_image_prefetch(user, message_id, url)
    }
}

/// Queue the remote images of a delivered message.
pub(crate) fn queue(
    state: &FastcoreState,
    user: &str,
    message_id: &str,
    parsed: &mailrs_mime::Part<'_>,
) {
    shared::queue(state, user, message_id, parsed);
}

/// Queue the remote images of `user`'s thread, for a thread taken out of
/// Junk or Review.
pub(crate) fn queue_thread(state: &Arc<FastcoreState>, user: &str, thread_id: &str) {
    if shared::fetcher().is_none() {
        return;
    }
    let raws = crate::bayes_train::fetch_thread_raw(state, user, thread_id);
    shared::queue_thread(state.as_ref(), user, thread_id, &raws);
}

/// Start the worker and the cache sweep. Off without `MAILRS_FETCH_URL`.
pub(crate) fn spawn(state: Arc<FastcoreState>) {
    shared::spawn(state);
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTML_MAIL: &[u8] = b"From: Shop <news@shop.example>\r\n\
To: bob@golia.jp\r\n\
Subject: Sale\r\n\
Message-ID: <sale1@shop.example>\r\n\
Date: Tue, 5 Aug 2026 09:00:00 +0900\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<p>Sale</p><img src=\"https://cdn.shop.example/banner.png\">\r\n";

    fn pending(state: &crate::FastcoreState) -> Vec<String> {
        state
            .mailbox
            .pending_image_prefetches(100)
            .unwrap()
            .into_iter()
            .map(|(_, _, url)| url)
            .collect()
    }

    /// Mail that arrives in the inbox has its images queued; mail that
    /// arrives in Junk does not, until the thread is taken out of Junk.
    /// One test: both halves set the same process environment.
    #[tokio::test]
    async fn images_are_queued_on_arrival_and_after_junk_only_when_released() {
        let maildir = tempfile::tempdir().unwrap();
        // SAFETY: test-only; nothing else in this binary reads these two
        unsafe {
            std::env::set_var("MAILRS_FETCH_URL", "http://fetch.invalid/fetch");
            std::env::set_var("MAILRS_MAILDIR", maildir.path());
        }
        let inbox = crate::tests::fresh_state();
        crate::ingest::ingest_delivered_file(
            &inbox,
            "bob@golia.jp",
            "s1.eml",
            HTML_MAIL,
            "INBOX",
            None,
        );
        assert_eq!(pending(&inbox), ["https://cdn.shop.example/banner.png"]);

        let junk = crate::tests::fresh_state();
        let new = maildir.path().join("golia.jp/bob/new");
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("s2.eml"), HTML_MAIL).unwrap();
        crate::ingest::ingest_delivered_file(
            &junk,
            "bob@golia.jp",
            "s2.eml",
            HTML_MAIL,
            "Junk",
            None,
        );
        assert!(pending(&junk).is_empty(), "mail in Junk was fetched for");
        queue_thread(&junk, "bob@golia.jp", "sale1@shop.example");
        assert_eq!(pending(&junk), ["https://cdn.shop.example/banner.png"]);

        // Each way a person takes a thread out of Junk or Review queues
        // its images: "not junk", "move to inbox" and releasing it from
        // Review. The last two did not, so those threads never showed one.
        use axum::extract::{Path as P, State};
        let new = maildir.path().join("golia.jp/bob/new");
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("s3.eml"), HTML_MAIL).unwrap();
        let (user, tid) = ("bob@golia.jp".to_string(), "sale1@shop.example".to_string());
        for way in ["not junk", "move to inbox", "release"] {
            let state = crate::tests::fresh_state();
            crate::ingest::ingest_delivered_file(&state, &user, "s3.eml", HTML_MAIL, "Junk", None);
            assert!(pending(&state).is_empty(), "{way}");
            let path = P((user.clone(), tid.clone()));
            match way {
                "not junk" => {
                    crate::routes::thread_actions::mark_not_junk(State(state.clone()), path).await;
                }
                "move to inbox" => {
                    crate::routes::thread_actions::move_to_inbox(State(state.clone()), path).await;
                }
                _ => {
                    state.mailbox.set_quarantined(&user, &tid, true).unwrap();
                    crate::release_quarantined(State(state.clone()), path).await;
                }
            }
            assert_eq!(
                pending(&state),
                ["https://cdn.shop.example/banner.png"],
                "{way}"
            );
        }
    }
}
