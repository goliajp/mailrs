//! Remote-synced sender-domain whitelist for greylisting.
//!
//! Pulls a plain-text list from a URL (default
//! `https://raw.githubusercontent.com/goliajp/mailrs/develop/assets/greylist-whitelist.txt`)
//! at startup and re-pulls every `MAILRS_GREYLIST_SYNC_INTERVAL_SECS`
//! (default 3600s). Domains in the list bypass the greylist stage —
//! the inbound pipeline's `GreylistStage::evaluate` checks this handle
//! before doing the per-triplet DB lookup.
//!
//! Phase 1 only does the remote whitelist; Phase 2 will layer a
//! PG-backed local white/black list on top (local always wins).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

/// In-memory snapshot of the latest sync result.
#[derive(Default, Debug, Clone)]
pub struct GreylistLists {
    /// Sender domains pulled from the remote whitelist URL, lowercased.
    pub remote_white: HashSet<String>,
    /// Last successful sync — unix epoch seconds. `None` until first sync.
    pub last_sync_at: Option<u64>,
    /// Most recent sync error (if any), reset to `None` on next success.
    pub last_error: Option<String>,
}

impl GreylistLists {
    /// Suffix-aware match: `domain.is_whitelisted("mail.gmail.com")` returns
    /// true when the whitelist contains `gmail.com` (or any ancestor).
    /// Empty / no-`@` senders are silently ignored.
    pub fn is_whitelisted(&self, sender_domain: &str) -> bool {
        if sender_domain.is_empty() || self.remote_white.is_empty() {
            return false;
        }
        let d = sender_domain.to_lowercase();
        if self.remote_white.contains(&d) {
            return true;
        }
        // ancestor walk: a.b.c.d → b.c.d → c.d → d. Stops when no more dots.
        let mut rest = d.as_str();
        while let Some(idx) = rest.find('.') {
            rest = &rest[idx + 1..];
            if self.remote_white.contains(rest) {
                return true;
            }
        }
        false
    }
}

/// Shareable handle to the latest [`GreylistLists`] snapshot.
pub type GreylistListsHandle = Arc<RwLock<GreylistLists>>;

/// Empty handle. Use when no remote URL is configured (sync disabled).
pub fn empty() -> GreylistListsHandle {
    Arc::new(RwLock::new(GreylistLists::default()))
}

/// Spawn a tokio task that fetches the URL once immediately, then every
/// `interval_secs`. Errors are recorded on the handle but never panic.
pub fn spawn_sync_task(
    handle: GreylistListsHandle,
    url: String,
    interval_secs: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // First sync runs immediately so the in-memory snapshot has
        // a real value before any mail arrives.
        sync_once(&handle, &url).await;
        let mut tick = tokio::time::interval(Duration::from_secs(interval_secs));
        // The first tick fires instantly — skip it (we already synced above)
        // so the actual cadence is interval_secs apart.
        tick.tick().await;
        loop {
            tick.tick().await;
            sync_once(&handle, &url).await;
        }
    })
}

async fn sync_once(handle: &GreylistListsHandle, url: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let outcome = fetch_and_parse(url).await;
    let mut g = handle.write().await;
    match outcome {
        Ok(set) => {
            let count = set.len();
            g.remote_white = set;
            g.last_sync_at = Some(now);
            g.last_error = None;
            tracing::info!(
                target: "greylist.sync",
                url,
                count,
                "greylist whitelist synced"
            );
            metrics::gauge!("mailrs_greylist_whitelist_size").set(count as f64);
            metrics::counter!("mailrs_greylist_sync_total", "outcome" => "ok").increment(1);
        }
        Err(e) => {
            let msg = e.to_string();
            tracing::warn!(
                target: "greylist.sync",
                url,
                error = %msg,
                "greylist whitelist sync failed"
            );
            g.last_error = Some(msg);
            metrics::counter!("mailrs_greylist_sync_total", "outcome" => "error").increment(1);
        }
    }
}

async fn fetch_and_parse(url: &str) -> Result<HashSet<String>, reqwest::Error> {
    let body = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(parse_list(&body))
}

/// Parse the plain-text wire format. Lowercases, strips comments, drops
/// blanks. Trailing-comment-on-data line is supported.
fn parse_list(text: &str) -> HashSet<String> {
    text.lines()
        .map(|l| match l.find('#') {
            Some(idx) => &l[..idx],
            None => l,
        })
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_list() {
        let text = "\
gmail.com
googlemail.com
# comment
outlook.com
";
        let s = parse_list(text);
        assert_eq!(s.len(), 3);
        assert!(s.contains("gmail.com"));
        assert!(s.contains("googlemail.com"));
        assert!(s.contains("outlook.com"));
    }

    #[test]
    fn parse_handles_trailing_comments_and_case_and_whitespace() {
        let text = "  Gmail.COM   # google\n\n  YAHOO.com\t# yahoo\n";
        let s = parse_list(text);
        assert_eq!(s.len(), 2);
        assert!(s.contains("gmail.com"));
        assert!(s.contains("yahoo.com"));
    }

    #[test]
    fn parse_empty_and_comment_only_returns_empty() {
        let s = parse_list("\n\n# just a comment\n   \n");
        assert!(s.is_empty());
    }

    #[test]
    fn is_whitelisted_direct_match() {
        let mut g = GreylistLists::default();
        g.remote_white.insert("gmail.com".into());
        assert!(g.is_whitelisted("gmail.com"));
        assert!(!g.is_whitelisted("outlook.com"));
    }

    #[test]
    fn is_whitelisted_suffix_ancestor() {
        let mut g = GreylistLists::default();
        g.remote_white.insert("gmail.com".into());
        // mail.gmail.com → gmail.com (ancestor) → hit
        assert!(g.is_whitelisted("mail.gmail.com"));
        assert!(g.is_whitelisted("smtp.relay.mail.gmail.com"));
    }

    #[test]
    fn is_whitelisted_case_insensitive() {
        let mut g = GreylistLists::default();
        g.remote_white.insert("gmail.com".into());
        assert!(g.is_whitelisted("GMAIL.COM"));
        assert!(g.is_whitelisted("Mail.Gmail.Com"));
    }

    #[test]
    fn is_whitelisted_unrelated_domain_misses() {
        let mut g = GreylistLists::default();
        g.remote_white.insert("gmail.com".into());
        // notgmail.com must NOT match gmail.com (only ancestor suffix counts)
        assert!(!g.is_whitelisted("notgmail.com"));
        assert!(!g.is_whitelisted("spammer.com"));
        assert!(!g.is_whitelisted("gmail.com.spammer.example"));
    }

    #[test]
    fn is_whitelisted_empty_inputs_are_safe() {
        let g = GreylistLists::default();
        assert!(!g.is_whitelisted("gmail.com"));
        let mut g = GreylistLists::default();
        g.remote_white.insert("gmail.com".into());
        assert!(!g.is_whitelisted(""));
    }
}
