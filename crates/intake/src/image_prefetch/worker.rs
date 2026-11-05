//! The fetching worker and the cache sweep.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mailrs_core_sidestate::image_cache;

use super::{ASKED, Lane, PrefetchQueue, WAKE, fetcher, host_of};

const BATCH: usize = 8;
/// Asked-for images looked at in a round: enough that a reader asking for
/// one image is seen next to a reader asking for fifty.
const ASKED_SCAN: usize = 64;
const IDLE: Duration = Duration::from_secs(5);

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
pub fn spawn(queue: Arc<dyn PrefetchQueue>) {
    let Some(url) = fetcher() else {
        tracing::info!("image prefetch off: MAILRS_FETCH_URL unset");
        return;
    };
    let cap = number("MAILRS_IMAGE_CACHE_MAX_BYTES", 5 * 1024 * 1024 * 1024);
    let root = image_cache::root();
    tokio::spawn(Worker::new(queue, url, root.clone()).run());
    tokio::spawn(sweep_forever(root, cap));
}

type Item = (String, String, String);

/// What one round fetched, by lane.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Done {
    pub asked: usize,
    pub background: usize,
}

/// Fetches queued images through the fetcher into the cache.
pub struct Worker {
    queue: Arc<dyn PrefetchQueue>,
    client: reqwest::Client,
    fetch_url: String,
    budget: Budget,
    spent: Mutex<Spent>,
    root: PathBuf,
}

impl Worker {
    /// A worker taking from `queue`, fetching through `fetch_url` into the
    /// cache at `root`, under the budget the environment sets.
    pub fn new(queue: Arc<dyn PrefetchQueue>, fetch_url: String, root: PathBuf) -> Self {
        let budget = Budget {
            per_domain_minute: number("MAILRS_IMAGE_PREFETCH_PER_DOMAIN_MINUTE", 30) as u32,
            per_day: number("MAILRS_IMAGE_PREFETCH_PER_DAY", 20_000) as u32,
        };
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("http client");
        Self {
            queue,
            client,
            fetch_url,
            budget,
            spent: Mutex::new(Spent::default()),
            root,
        }
    }

    async fn run(self) {
        loop {
            let done = self.round().await;
            if done.asked > 0 {
                continue;
            }
            // everything left is over budget: wait rather than spin on it
            let pause = match done.background {
                0 => IDLE,
                _ => Duration::from_millis(200),
            };
            tokio::select! {
                () = tokio::time::sleep(pause) => {}
                () = WAKE.notified() => {}
            }
        }
    }

    /// One round: the images readers asked for, a few of each reader's in
    /// turn; when none of those could be fetched, the background lane, one
    /// image at a time, stopping as soon as a reader asks.
    pub async fn round(&self) -> Done {
        // cleared before the asked lane is read, so an ask after the read
        // stops the background part of this round
        ASKED.store(false, Ordering::SeqCst);
        let asked = fair(self.read(Lane::Asked, ASKED_SCAN), BATCH);
        let asked = self.fetch_all(Lane::Asked, asked).await;
        if asked > 0 {
            return Done {
                asked,
                background: 0,
            };
        }
        let background = self.read(Lane::Background, BATCH);
        Done {
            asked: 0,
            background: self.fetch_all(Lane::Background, background).await,
        }
    }

    fn read(&self, lane: Lane, limit: usize) -> Vec<Item> {
        match self.queue.pending(lane, limit) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(error = %e, ?lane, "image prefetch queue unreadable");
                Vec::new()
            }
        }
    }

    /// Fetch what of `items` the budget allows; how many were fetched.
    async fn fetch_all(&self, lane: Lane, items: Vec<Item>) -> usize {
        let mut taken = 0;
        for (user, message_id, url) in items {
            if lane == Lane::Background && ASKED.load(Ordering::SeqCst) {
                break;
            }
            let domain = host_of(&url).to_ascii_lowercase();
            let now = chrono::Utc::now().timestamp();
            let allowed = self
                .spent
                .lock()
                .map(|mut s| s.take(self.budget, &domain, now))
                .unwrap_or(false);
            if !allowed {
                // over budget: left for a later round
                continue;
            }
            taken += 1;
            fetch_one(&self.client, &self.fetch_url, &self.root, &user, &url).await;
            if let Err(e) = self.queue.done(lane, &user, &message_id, &url) {
                tracing::warn!(error = %e, "image prefetch not taken off the queue");
            }
        }
        taken
    }
}

/// Up to `n` of `items`, one from each user in turn, so one reader's long
/// thread does not keep another reader waiting.
fn fair(items: Vec<Item>, n: usize) -> Vec<Item> {
    let mut by_user: Vec<VecDeque<Item>> = Vec::new();
    for item in items {
        match by_user.iter_mut().find(|q| q[0].0 == item.0) {
            Some(q) => q.push_back(item),
            None => by_user.push(VecDeque::from([item])),
        }
    }
    let mut out = Vec::new();
    while out.len() < n && by_user.iter().any(|q| !q.is_empty()) {
        for q in &mut by_user {
            if out.len() < n
                && let Some(item) = q.pop_front()
            {
                out.push(item);
            }
        }
    }
    out
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
    fn each_user_has_a_directory_of_their_own() {
        let root = Path::new("/c");
        let a = image_cache::path(root, "A@x.jp", "https://e/x.png");
        assert_eq!(a, image_cache::path(root, "a@x.jp", "https://e/x.png"));
        assert_ne!(
            a.parent(),
            image_cache::path(root, "b@x.jp", "https://e/x.png").parent()
        );
    }

    /// A reader asking for one image is served in the same round as a
    /// reader asking for fifty, not after them.
    #[test]
    fn asked_images_are_taken_from_each_reader_in_turn() {
        let item = |u: &str, n: usize| (u.to_string(), "m".to_string(), format!("https://e/{n}"));
        let mut items: Vec<Item> = (0..50).map(|n| item("a", n)).collect();
        items.push(item("b", 0));
        items.extend((50..52).map(|n| item("a", n)));
        items.push(item("c", 0));
        let users: Vec<String> = fair(items, 5).into_iter().map(|i| i.0).collect();
        assert_eq!(users, ["a", "b", "c", "a", "a"]);
        assert_eq!(fair(vec![item("a", 0)], 8).len(), 1);
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
