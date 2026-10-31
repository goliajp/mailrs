//! The fetching worker and the cache sweep.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mailrs_core_sidestate::image_cache;

use super::{PrefetchQueue, fetcher, host_of};

const BATCH: usize = 8;
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
    let budget = Budget {
        per_domain_minute: number("MAILRS_IMAGE_PREFETCH_PER_DOMAIN_MINUTE", 30) as u32,
        per_day: number("MAILRS_IMAGE_PREFETCH_PER_DAY", 20_000) as u32,
    };
    let cap = number("MAILRS_IMAGE_CACHE_MAX_BYTES", 5 * 1024 * 1024 * 1024);
    let root = image_cache::root();
    tokio::spawn(work(queue, url, budget, root.clone()));
    tokio::spawn(sweep_forever(root, cap));
}

async fn work(queue: Arc<dyn PrefetchQueue>, fetch_url: String, budget: Budget, root: PathBuf) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("http client");
    let spent = Mutex::new(Spent::default());
    loop {
        let batch = match queue.pending(BATCH) {
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
        let mut taken = 0;
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
            taken += 1;
            fetch_one(&client, &fetch_url, &root, &user, &url).await;
            if let Err(e) = queue.done(&user, &message_id, &url) {
                tracing::warn!(error = %e, "image prefetch not taken off the queue");
            }
        }
        // everything left is over budget: wait rather than spin on it
        let pause = match taken {
            0 => IDLE,
            _ => Duration::from_millis(200),
        };
        tokio::time::sleep(pause).await;
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
    fn each_user_has_a_directory_of_their_own() {
        let root = Path::new("/c");
        let a = image_cache::path(root, "A@x.jp", "https://e/x.png");
        assert_eq!(a, image_cache::path(root, "a@x.jp", "https://e/x.png"));
        assert_ne!(
            a.parent(),
            image_cache::path(root, "b@x.jp", "https://e/x.png").parent()
        );
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
