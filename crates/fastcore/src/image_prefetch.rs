//! Remote images in a delivered message, fetched ahead of time.
//!
//! A sender who puts an image in a message learns, when it is fetched, who
//! opened the mail and when. Fetching it as the mail arrives tells the
//! sender only that it arrived. Delivery queues the image URLs of a message
//! that did not go to Junk (`queue`); the worker hands each to the fetcher
//! process (`mailrs-fetch`, which alone talks to the outside and refuses
//! every way inside) and keeps what comes back under the recipient's own
//! directory, named by the URL's hash. The web client asks for an image by
//! that hash and gets one only from its own user's directory.
//!
//! Bounded: 50 images a message, 30 a minute per sending domain, 20000 a
//! day in all, and the cache under a size cap, oldest-read first out.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mailrs_core_sidestate::image_cache;

use crate::FastcoreState;

const PER_MESSAGE: usize = 50;
const BATCH: usize = 8;
const IDLE: Duration = Duration::from_secs(5);

/// The fetcher's address; prefetching is off without one.
fn fetcher() -> Option<String> {
    std::env::var("MAILRS_FETCH_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
}

/// Queue the remote images of `user`'s new message. Called on delivery,
/// not for mail that went to Junk.
pub(crate) fn queue(
    state: &FastcoreState,
    user: &str,
    message_id: &str,
    parsed: &mailrs_mime::Part<'_>,
) {
    if fetcher().is_none() {
        return;
    }
    let mut urls = Vec::new();
    for part in parsed
        .walk()
        .filter(|p| p.content_type.mime_type() == "text/html")
    {
        if let Some(html) = part.body_text() {
            for url in image_urls(&html) {
                if !urls.contains(&url) && urls.len() < PER_MESSAGE {
                    urls.push(url);
                }
            }
        }
    }
    if let Err(e) = state.mailbox.queue_image_prefetch(user, message_id, &urls) {
        tracing::warn!(error = %e, %user, "queueing image prefetch failed");
    }
}

/// Queue the remote images of every message in `user`'s thread: for a
/// thread taken out of Junk, whose images were not fetched on arrival.
pub(crate) fn queue_thread(state: &Arc<FastcoreState>, user: &str, thread_id: &str) {
    if fetcher().is_none() {
        return;
    }
    for (n, raw) in crate::bayes_train::fetch_thread_raw(state, user, thread_id)
        .iter()
        .enumerate()
    {
        // the queue keys an image by message only to tell entries apart
        queue(
            state,
            user,
            &format!("{thread_id}#{n}"),
            &mailrs_mime::parse(raw),
        );
    }
}

/// The `src` of every `<img>` that names an http or https URL, as the
/// browser will read it: HTML entities decoded.
pub fn image_urls(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = lower[from..].find("<img") {
        let start = from + at;
        let end = lower[start..].find('>').map_or(lower.len(), |e| start + e);
        from = end;
        let tag = &html[start..end];
        let tag_lower = &lower[start..end];
        let Some(src_at) = find_attr(tag_lower, "src") else {
            continue;
        };
        let rest = tag[src_at..].trim_start();
        let value = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or(""),
            _ => rest
                .split(|c: char| c.is_whitespace() || c == '>')
                .next()
                .unwrap_or(""),
        };
        let url = unescape(value.trim());
        let scheme = url
            .get(..8)
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if scheme.starts_with("http://") || scheme.starts_with("https://") {
            out.push(url);
        }
    }
    out
}

/// The offset just past `name=` in a lowercased tag, at a word boundary.
fn find_attr(tag: &str, name: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(at) = tag[from..].find(name) {
        let start = from + at;
        let before_ok = tag[..start].ends_with(|c: char| c.is_whitespace());
        let after = tag[start + name.len()..].trim_start();
        if before_ok && after.starts_with('=') {
            let eq = tag.len() - after.len();
            return Some(eq + 1);
        }
        from = start + name.len();
    }
    None
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let Some(semi) = tail.find(';').filter(|i| *i <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..semi];
        let ch = match entity {
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "lt" => Some('<'),
            "gt" => Some('>'),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The host of an http(s) URL, for the per-domain budget.
fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    host.split(':').next().unwrap_or("")
}

/// Fetches allowed per sending domain per minute, and in all per day.
#[derive(Debug, Clone, Copy)]
struct Budget {
    per_domain_minute: u32,
    per_day: u32,
}

#[derive(Default)]
struct Spent {
    minute: i64,
    by_domain: HashMap<String, u32>,
    day: i64,
    today: u32,
}

impl Spent {
    /// Whether a fetch from `domain` at `now` is within `budget`; counts it.
    fn take(&mut self, budget: Budget, domain: &str, now: i64) -> bool {
        if now / 60 != self.minute {
            self.minute = now / 60;
            self.by_domain.clear();
        }
        if now / 86_400 != self.day {
            self.day = now / 86_400;
            self.today = 0;
        }
        let n = self.by_domain.entry(domain.to_string()).or_default();
        if *n >= budget.per_domain_minute || self.today >= budget.per_day {
            return false;
        }
        *n += 1;
        self.today += 1;
        true
    }
}

fn number(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// Start the worker and the cache sweep. Off without `MAILRS_FETCH_URL`.
pub(crate) fn spawn(state: Arc<FastcoreState>) {
    let Some(url) = fetcher() else {
        tracing::info!("image prefetch off: MAILRS_FETCH_URL unset");
        return;
    };
    let budget = Budget {
        per_domain_minute: number("MAILRS_IMAGE_PREFETCH_PER_DOMAIN_MINUTE", 30) as u32,
        per_day: number("MAILRS_IMAGE_PREFETCH_PER_DAY", 20_000) as u32,
    };
    let cap = number("MAILRS_IMAGE_CACHE_MAX_BYTES", 5 * 1024 * 1024 * 1024);
    let root = image_cache::root();
    tokio::spawn(work(state, url, budget, root.clone()));
    tokio::spawn(sweep_forever(root, cap));
}

async fn work(state: Arc<FastcoreState>, fetch_url: String, budget: Budget, root: PathBuf) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("http client");
    let spent = Mutex::new(Spent::default());
    loop {
        let batch = match state.mailbox.pending_image_prefetches(BATCH) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(error = %e, "image prefetch queue unreadable");
                Vec::new()
            }
        };
        if batch.is_empty() {
            tokio::time::sleep(IDLE).await;
            continue;
        }
        for (user, message_id, url) in batch {
            let domain = host_of(&url).to_ascii_lowercase();
            let now = chrono::Utc::now().timestamp();
            let allowed = spent
                .lock()
                .map(|mut s| s.take(budget, &domain, now))
                .unwrap_or(false);
            if !allowed {
                // over budget: left for a later round
                continue;
            }
            fetch_one(&client, &fetch_url, &root, &user, &url).await;
            if let Err(e) = state.mailbox.done_image_prefetch(&user, &message_id, &url) {
                tracing::warn!(error = %e, "image prefetch not taken off the queue");
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn fetch_one(client: &reqwest::Client, fetch_url: &str, root: &Path, user: &str, url: &str) {
    let path = image_cache::path(root, user, url);
    if path.exists() {
        return;
    }
    let resp = client
        .post(fetch_url)
        .json(&serde_json::json!({ "url": url }))
        .send()
        .await;
    let bytes = match resp {
        Ok(r) if r.status().is_success() => match r.bytes().await {
            Ok(b) => b,
            Err(_) => return,
        },
        // refused or failed: no image, and nothing to retry
        _ => return,
    };
    let tmp = path.with_extension("partial");
    let written = std::fs::create_dir_all(path.parent().unwrap_or(root))
        .and_then(|()| std::fs::write(&tmp, &bytes))
        .and_then(|()| std::fs::rename(&tmp, &path));
    if let Err(e) = written {
        tracing::warn!(error = %e, "image cache write failed");
    }
}

async fn sweep_forever(root: PathBuf, cap: u64) {
    loop {
        let r = root.clone();
        let removed =
            tokio::task::spawn_blocking(move || sweep(&r, cap, std::time::SystemTime::now()))
                .await
                .unwrap_or(0);
        if removed > 0 {
            tracing::info!(removed, "image cache swept");
        }
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

/// Days an unread image stays.
const KEEP: Duration = Duration::from_secs(30 * 86_400);

/// Drop images not read for 30 days, then the least recently read until
/// the cache is under `cap` bytes. Reading an image touches its mtime.
/// Walks the whole cache: it is bounded by `cap`, a few thousand files.
pub fn sweep(root: &Path, cap: u64, now: std::time::SystemTime) -> usize {
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
    for dir in std::fs::read_dir(root).into_iter().flatten().flatten() {
        for f in std::fs::read_dir(dir.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            if let Ok(meta) = f.metadata()
                && let Ok(m) = meta.modified()
                && meta.is_file()
            {
                files.push((m, meta.len(), f.path()));
            }
        }
    }
    files.sort();
    let mut total: u64 = files.iter().map(|(_, n, _)| n).sum();
    let mut removed = 0;
    for (modified, size, path) in files {
        let stale = now.duration_since(modified).is_ok_and(|age| age > KEEP);
        if (stale || total > cap) && std::fs::remove_file(&path).is_ok() {
            total -= size;
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_sources_are_read_as_a_browser_reads_them() {
        let html = r#"<p><IMG width=1 SRC="https://t.example/o.gif?u=1&amp;m=2"></p>
            <img alt='x' src='http://cdn.example/a.png'>
            <img data-src="https://no.example/x.png" src=https://bare.example/b.jpg>
            <img src="cid:part1@x"> <img src="data:image/png;base64,AAAA">
            <a href="https://link.example/">x</a>"#;
        assert_eq!(
            image_urls(html),
            vec![
                "https://t.example/o.gif?u=1&m=2",
                "http://cdn.example/a.png",
                "https://bare.example/b.jpg",
            ]
        );
    }

    #[test]
    fn each_user_has_a_directory_of_their_own() {
        let root = Path::new("/c");
        let a = image_cache::path(root, "A@x.jp", "https://e/x.png");
        assert_eq!(a, image_cache::path(root, "a@x.jp", "https://e/x.png"));
        assert_ne!(
            a.parent(),
            image_cache::path(root, "b@x.jp", "https://e/x.png").parent()
        );
    }

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
    #[test]
    fn images_are_queued_on_arrival_and_after_junk_only_when_released() {
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
    }

    #[test]
    fn the_budget_counts_by_host() {
        assert_eq!(
            host_of("https://Img.Example.com:443/a.png?x=1"),
            "Img.Example.com"
        );
        assert_eq!(host_of("http://u@e.example/x"), "e.example");
    }

    #[test]
    fn the_budget_holds_per_domain_and_per_day() {
        let budget = Budget {
            per_domain_minute: 2,
            per_day: 3,
        };
        let mut s = Spent::default();
        assert!(s.take(budget, "a", 60));
        assert!(s.take(budget, "a", 61));
        assert!(
            !s.take(budget, "a", 62),
            "a third from one domain in a minute"
        );
        assert!(s.take(budget, "b", 63));
        assert!(!s.take(budget, "c", 64), "the day's total");
        assert!(s.take(budget, "c", 86_400 * 2), "a new day");
    }

    #[test]
    fn the_sweep_drops_the_stale_and_then_the_least_read() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("u");
        std::fs::create_dir_all(&user).unwrap();
        let now = std::time::SystemTime::now();
        for (name, age_days) in [("old", 40u64), ("mid", 2), ("new", 1)] {
            let p = user.join(name);
            std::fs::write(&p, vec![0u8; 100]).unwrap();
            let t = now - Duration::from_secs(age_days * 86_400);
            std::fs::File::options()
                .write(true)
                .open(&p)
                .unwrap()
                .set_modified(t)
                .unwrap();
        }
        // stale "old" goes; then 200 bytes against a cap of 150: "mid" goes
        assert_eq!(sweep(dir.path(), 150, now), 2);
        assert!(user.join("new").exists());
        assert!(!user.join("mid").exists() && !user.join("old").exists());
    }
}
