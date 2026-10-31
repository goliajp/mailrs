//! Remote images in a delivered message, fetched ahead of time.
//!
//! A sender who puts an image in a message learns, when it is fetched, who
//! opened the mail and when. Fetching it as the mail arrives tells the
//! sender only that it arrived. Each core queues the image URLs of a
//! message that did not go to Junk and was not held, and again when a
//! person takes a thread out of Junk or Review; the worker hands each URL
//! to the fetcher process (`mailrs-fetch`, which alone talks to the outside
//! and refuses every way inside) and keeps what comes back under the
//! recipient's own directory, named by the URL's hash. The web client asks
//! for an image by that hash and gets one only from its own user's
//! directory.
//!
//! The queue is the core's own: fastcore keeps it in its embedded store,
//! the SQL core in the network kevy. Everything else is here, once.
//!
//! Bounded: 50 images a message, 30 a minute per sending domain, 20000 a
//! day in all, and the cache under a size cap, oldest-read first out.

mod worker;

use std::io;

pub use worker::{spawn, sweep};

const PER_MESSAGE: usize = 50;

/// Where a core keeps the images waiting to be fetched.
pub trait PrefetchQueue: Send + Sync + 'static {
    /// Queue `urls`, found in `user`'s message `message_id`.
    fn queue(&self, user: &str, message_id: &str, urls: &[String]) -> io::Result<()>;
    /// Up to `limit` queued `(user, message id, url)`, left in the queue.
    fn pending(&self, limit: usize) -> io::Result<Vec<(String, String, String)>>;
    /// Take one fetched or given-up image off the queue.
    fn done(&self, user: &str, message_id: &str, url: &str) -> io::Result<()>;
}

/// The fetcher's address; prefetching is off without one.
pub fn fetcher() -> Option<String> {
    std::env::var("MAILRS_FETCH_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
}

/// Queue the remote images of `user`'s new message. Called on delivery,
/// not for mail that went to Junk or was held.
pub fn queue(q: &dyn PrefetchQueue, user: &str, message_id: &str, parsed: &mailrs_mime::Part<'_>) {
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
    if let Err(e) = q.queue(user, message_id, &urls) {
        tracing::warn!(error = %e, %user, "queueing image prefetch failed");
    }
}

/// Queue the remote images of every message of `user`'s thread, given as
/// raw messages: for a thread taken out of Junk or Review, whose images
/// were not fetched on arrival. An image already in the cache is not
/// fetched again.
pub fn queue_thread(q: &dyn PrefetchQueue, user: &str, thread_id: &str, raws: &[Vec<u8>]) {
    if fetcher().is_none() {
        return;
    }
    for (n, raw) in raws.iter().enumerate() {
        // the queue keys an image by message only to tell entries apart
        queue(
            q,
            user,
            &format!("{thread_id}#{n}"),
            &mailrs_mime::parse(raw),
        );
    }
}

/// The queue in the network kevy, for a core without a store of its own
/// to keep it in.
pub struct NetworkQueue {
    pub url: String,
}

impl NetworkQueue {
    fn conn(&self) -> io::Result<kevy_client::Connection> {
        Ok(kevy_client::Connection::connect(&self.url)?)
    }
}

impl PrefetchQueue for NetworkQueue {
    fn queue(&self, user: &str, message_id: &str, urls: &[String]) -> io::Result<()> {
        mailrs_core_sidestate::families::image_prefetch::queue(
            &mut self.conn()?,
            user,
            message_id,
            urls,
        )
    }
    fn pending(&self, limit: usize) -> io::Result<Vec<(String, String, String)>> {
        mailrs_core_sidestate::families::image_prefetch::pending(&mut self.conn()?, limit)
    }
    fn done(&self, user: &str, message_id: &str, url: &str) -> io::Result<()> {
        mailrs_core_sidestate::families::image_prefetch::done(
            &mut self.conn()?,
            user,
            message_id,
            url,
        )
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
    fn the_budget_counts_by_host() {
        assert_eq!(
            host_of("https://Img.Example.com:443/a.png?x=1"),
            "Img.Example.com"
        );
        assert_eq!(host_of("http://u@e.example/x"), "e.example");
    }
}
