//! fastcore's side of the remote-image prefetch: the queue lives in its
//! embedded store. What is queued, the worker and the cache are shared
//! with the SQL core in `mailrs_intake::image_prefetch`.

use std::io;
use std::sync::Arc;

use mailrs_intake::image_prefetch::{self as shared, Lane, PrefetchQueue};
use mailrs_mailbox_kevy::keys::{IMAGE_PREFETCH_ASKED, IMAGE_PREFETCH_PENDING};

use crate::FastcoreState;

fn set(lane: Lane) -> &'static [u8] {
    match lane {
        Lane::Background => IMAGE_PREFETCH_PENDING,
        Lane::Asked => IMAGE_PREFETCH_ASKED,
    }
}

impl PrefetchQueue for FastcoreState {
    fn queue(&self, lane: Lane, user: &str, message_id: &str, urls: &[String]) -> io::Result<()> {
        self.mailbox
            .queue_image_prefetch(set(lane), user, message_id, urls)
    }
    fn pending(&self, lane: Lane, limit: usize) -> io::Result<Vec<(String, String, String)>> {
        self.mailbox.pending_image_prefetches(set(lane), limit)
    }
    fn done(&self, lane: Lane, user: &str, message_id: &str, url: &str) -> io::Result<()> {
        self.mailbox
            .done_image_prefetch(set(lane), user, message_id, url)
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

/// Queue the remote images of `user`'s thread for a reader who has it open
/// and asked for them: served ahead of everything queued in the background.
pub(crate) fn ask_thread(state: &Arc<FastcoreState>, user: &str, thread_id: &str) {
    if shared::fetcher().is_none() {
        return;
    }
    let raws = crate::bayes_train::fetch_thread_raw(state, user, thread_id);
    shared::ask_thread(state.as_ref(), user, thread_id, &raws);
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
        in_lane(state, Lane::Background)
    }

    fn in_lane(state: &crate::FastcoreState, lane: Lane) -> Vec<String> {
        PrefetchQueue::pending(state, lane, 100)
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

        // A message once read is filed under its flags (`:2,S`); the
        // lookup by the bare name found no file for it, and nothing of the
        // mail the person had already opened was ever queued.
        let read = crate::tests::fresh_state();
        let cur = maildir.path().join("golia.jp/bob/cur");
        std::fs::create_dir_all(&cur).unwrap();
        std::fs::write(cur.join("s4.eml:2,S"), HTML_MAIL).unwrap();
        crate::ingest::ingest_delivered_file(
            &read,
            "bob@golia.jp",
            "s4.eml",
            HTML_MAIL,
            "Junk",
            None,
        );
        assert!(pending(&read).is_empty());
        queue_thread(&read, "bob@golia.jp", "sale1@shop.example");
        assert_eq!(
            pending(&read),
            ["https://cdn.shop.example/banner.png"],
            "a read message"
        );

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

        // A reader with the thread open asks for its images, here past
        // the Junk refusal: they wait in the asked lane, which the worker
        // serves first, and not behind everything queued on arrival.
        let state = crate::tests::fresh_state();
        crate::ingest::ingest_delivered_file(&state, &user, "s3.eml", HTML_MAIL, "Junk", None);
        let anyway =
            axum::extract::Query::try_from_uri(&"http://x/?anyway=true".parse().unwrap()).unwrap();
        let path = P((user.clone(), tid.clone()));
        crate::routes::images::prefetch_images(State(state.clone()), path, anyway).await;
        assert_eq!(
            in_lane(&state, Lane::Asked),
            ["https://cdn.shop.example/banner.png"]
        );
        assert!(pending(&state).is_empty(), "the background lane");
    }

    fn mail_with(image: &str) -> Vec<u8> {
        format!(
            "From: Shop <news@shop.example>\r\nTo: b@golia.jp\r\nSubject: x\r\n\
             Message-ID: <x@shop.example>\r\nContent-Type: text/html\r\n\r\n\
             <img src=\"{image}\">\r\n"
        )
        .into_bytes()
    }

    /// What a reader asks for is fetched before what arrival queued, even
    /// behind a backlog, and a reader who asks while the worker is in the
    /// middle of the backlog waits for one fetch, not for the rest of it.
    /// The fetcher is a local server standing in for `mailrs-fetch`, which
    /// refuses every local address; it records the order it was asked in.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn asked_images_are_fetched_ahead_of_the_backlog() {
        use std::sync::Mutex;
        // SAFETY: test-only; the same value as the other test in this binary
        unsafe { std::env::set_var("MAILRS_FETCH_URL", "http://fetch.invalid/fetch") };
        let state = crate::tests::fresh_state();
        let backlog: Vec<String> = (0..20)
            .map(|n| format!("https://backlog.example/{n}.png"))
            .collect();
        PrefetchQueue::queue(
            state.as_ref(),
            Lane::Background,
            "a@golia.jp",
            "<bulk@x>",
            &backlog,
        )
        .unwrap();
        let first = "https://first.example/1.png";
        let later = "https://later.example/1.png";
        shared::ask_thread(state.as_ref(), "b@golia.jp", "t-b", &[mail_with(first)]);

        let order = Arc::new(Mutex::new(Vec::<String>::new()));
        let app = {
            let (order, state) = (order.clone(), state.clone());
            axum::Router::new().route(
                "/fetch",
                axum::routing::post(move |axum::Json(req): axum::Json<serde_json::Value>| {
                    let (order, state) = (order.clone(), state.clone());
                    async move {
                        let url = req["url"].as_str().unwrap_or_default().to_string();
                        let mut seen = order.lock().unwrap();
                        // the second reader asks during the first backlog fetch
                        if url.contains("backlog") && !seen.iter().any(|u| u.contains("backlog")) {
                            shared::ask_thread(
                                state.as_ref(),
                                "c@golia.jp",
                                "t-c",
                                &[mail_with(later)],
                            );
                        }
                        seen.push(url);
                        b"\x89PNG\r\n\x1a\n".to_vec()
                    }
                }),
            )
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await });

        let cache = tempfile::tempdir().unwrap();
        let worker = shared::Worker::new(
            state.clone(),
            format!("http://{addr}/fetch"),
            cache.path().to_path_buf(),
        );
        // until both lanes are empty: another test's ask can end a round early
        for _ in 0..30 {
            worker.round().await;
            if in_lane(&state, Lane::Asked).is_empty() && pending(&state).is_empty() {
                break;
            }
        }
        let order = order.lock().unwrap().clone();
        assert_eq!(order.len(), 22, "every image fetched once: {order:?}");
        assert_eq!(order[0], first, "the asked image went first: {order:?}");
        assert!(order[1].contains("backlog"), "{order:?}");
        assert_eq!(
            order[2], later,
            "the second reader waited one fetch: {order:?}"
        );
        for (user, url) in [("b@golia.jp", first), ("c@golia.jp", later)] {
            let kept = mailrs_core_sidestate::image_cache::path(cache.path(), user, url);
            assert!(kept.exists(), "{url}");
        }
        assert!(in_lane(&state, Lane::Asked).is_empty());
        assert!(pending(&state).is_empty());
    }
}
