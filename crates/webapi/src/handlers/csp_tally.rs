//! What this process counted since it started: the content security policy
//! reports browsers sent, and the web pages it served, by client class.
//! The ops daemon gets the totals in every heartbeat and keeps them, so a
//! week of reports survives the restart each deploy makes.

use std::collections::BTreeMap;
use std::sync::Mutex;

use mailrs_ops_report::wire::{CSP_DISTINCT_MAX, CspEntry, WebapiBeat};

/// The distinct list stops taking new entries at this many bytes as well,
/// so a heartbeat stays inside the daemon's 64 KiB report limit with room
/// for the rest of the report.
const DISTINCT_BYTES_MAX: usize = 56 * 1024;
const BLOCKED_MAX_CHARS: usize = 100;
const PAGE_MAX_CHARS: usize = 40;

/// Directive names a policy can have. Anything else a report claims is
/// counted as `other`: the endpoint takes reports from anyone.
const DIRECTIVES: &[&str] = &[
    "base-uri",
    "child-src",
    "connect-src",
    "default-src",
    "fenced-frame-src",
    "font-src",
    "form-action",
    "frame-ancestors",
    "frame-src",
    "img-src",
    "manifest-src",
    "media-src",
    "object-src",
    "prefetch-src",
    "require-trusted-types-for",
    "sandbox",
    "script-src",
    "script-src-attr",
    "script-src-elem",
    "style-src",
    "style-src-attr",
    "style-src-elem",
    "trusted-types",
    "upgrade-insecure-requests",
    "worker-src",
];

type Key = (String, String, String, String, String);

struct Seen {
    count: u64,
    first_at: i64,
    last_at: i64,
}

#[derive(Default)]
struct Tally {
    reports: u64,
    by_directive: BTreeMap<String, u64>,
    distinct: BTreeMap<Key, Seen>,
    distinct_bytes: usize,
    dropped: u64,
    pages: BTreeMap<String, u64>,
}

static TALLY: Mutex<Option<Tally>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Tally) -> R) -> R {
    let mut guard = TALLY.lock().unwrap_or_else(|p| p.into_inner());
    f(guard.get_or_insert_with(Tally::default))
}

/// One report, already stripped to what may be kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub directive: String,
    pub violated_directive: String,
    pub blocked: String,
    pub page: String,
    pub client: &'static str,
}

impl Report {
    /// From a report's raw fields; `blocked` is already an origin or a
    /// keyword, `document` is the page URL as the browser sent it.
    pub fn new(
        directive: &str,
        violated_directive: &str,
        blocked: &str,
        document: &str,
        client: &'static str,
    ) -> Self {
        Report {
            directive: directive_name(directive),
            violated_directive: violated_directive.chars().take(200).collect(),
            blocked: blocked.chars().take(BLOCKED_MAX_CHARS).collect(),
            page: first_segment(document),
            client,
        }
    }
}

/// Count one report received at `now` (unix seconds).
pub fn record(r: Report, now: i64) {
    with(|t| t.record(r, now));
}

/// Count one web page served to a client with this `User-Agent`.
pub fn page_served(user_agent: Option<&str>) {
    with(|t| *t.pages.entry(client_class(user_agent).into()).or_default() += 1);
}

/// The totals as a heartbeat carries them.
pub fn beat() -> WebapiBeat {
    with(|t| t.beat())
}

impl Tally {
    fn record(&mut self, r: Report, now: i64) {
        self.reports += 1;
        *self.by_directive.entry(r.directive.clone()).or_default() += 1;
        let key: Key = (
            r.directive,
            r.violated_directive,
            r.blocked,
            r.page,
            r.client.to_string(),
        );
        if let Some(seen) = self.distinct.get_mut(&key) {
            seen.count += 1;
            seen.last_at = now;
            return;
        }
        let bytes = entry(
            &key,
            &Seen {
                count: u64::MAX,
                first_at: now,
                last_at: now,
            },
        )
        .len();
        if self.distinct.len() >= CSP_DISTINCT_MAX
            || self.distinct_bytes + bytes > DISTINCT_BYTES_MAX
        {
            self.dropped += 1;
            return;
        }
        self.distinct_bytes += bytes;
        self.distinct.insert(
            key,
            Seen {
                count: 1,
                first_at: now,
                last_at: now,
            },
        );
    }

    fn beat(&self) -> WebapiBeat {
        WebapiBeat {
            csp_reports: self.reports,
            csp_by_directive: self.by_directive.clone(),
            csp_distinct: self.distinct.iter().map(|(k, s)| to_entry(k, s)).collect(),
            csp_distinct_dropped: self.dropped,
            pages_served: self.pages.clone(),
        }
    }
}

fn to_entry((d, v, b, p, c): &Key, s: &Seen) -> CspEntry {
    CspEntry {
        directive: d.clone(),
        violated_directive: v.clone(),
        blocked: b.clone(),
        page: p.clone(),
        client: c.clone(),
        count: s.count,
        first_at: s.first_at,
        last_at: s.last_at,
    }
}

/// An entry as the heartbeat will carry it, escapes and all.
fn entry(key: &Key, s: &Seen) -> Vec<u8> {
    serde_json::to_vec(&to_entry(key, s)).expect("strings and numbers serialise")
}

/// The class of client a `User-Agent` names.
pub fn client_class(user_agent: Option<&str>) -> &'static str {
    let ua = user_agent.unwrap_or("").trim();
    if ua.is_empty() {
        return "unknown";
    }
    if ua.contains("; wv)") {
        return "android-webview";
    }
    if ua.contains("Mobile") {
        return "mobile";
    }
    "desktop"
}

/// The directive's name, or `other` when no policy has one by that name.
/// A CSP2 `violated-directive` carries its sources after the name.
fn directive_name(raw: &str) -> String {
    let name = raw.split_whitespace().next().unwrap_or("");
    match DIRECTIVES.contains(&name) {
        true => name.to_string(),
        false => "other".to_string(),
    }
}

/// The first segment of a URL's path: `mail` for `/mail/thread/42`, `""`
/// for the root. App paths carry thread ids, so nothing past it is kept.
fn first_segment(url: &str) -> String {
    let path = match url.split_once("://") {
        Some((_, rest)) => rest.find('/').map_or("", |i| &rest[i..]),
        None => url,
    };
    let path = path.split(['?', '#']).next().unwrap_or("");
    path.trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or("")
        .chars()
        .take(PAGE_MAX_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(blocked: &str) -> Report {
        Report::new(
            "img-src",
            "img-src",
            blocked,
            "https://mail.golia.ai/mail/thread/42?x=1",
            "desktop",
        )
    }

    #[test]
    fn the_same_report_twice_is_one_entry_counted_twice() {
        let mut t = Tally::default();
        t.record(report("https://a.example"), 10);
        t.record(report("https://a.example"), 20);
        t.record(report("https://b.example"), 30);
        let b = t.beat();
        assert_eq!(b.csp_reports, 3);
        assert_eq!(b.csp_by_directive["img-src"], 3);
        assert_eq!(b.csp_distinct.len(), 2);
        let a = &b.csp_distinct[0];
        assert_eq!((a.count, a.first_at, a.last_at), (2, 10, 20));
        assert_eq!(a.page, "mail");
        assert_eq!(b.csp_distinct_dropped, 0);
    }

    #[test]
    fn a_full_list_counts_new_keys_as_dropped_and_old_keys_still_grow() {
        let mut t = Tally::default();
        for i in 0..CSP_DISTINCT_MAX + 5 {
            t.record(report(&format!("https://h{i}.example")), 1);
        }
        t.record(report("https://h0.example"), 2);
        let b = t.beat();
        assert_eq!(b.csp_distinct.len(), CSP_DISTINCT_MAX);
        assert_eq!(b.csp_distinct_dropped, 5);
        assert_eq!(b.csp_reports, CSP_DISTINCT_MAX as u64 + 6);
        let h0 = b
            .csp_distinct
            .iter()
            .find(|e| e.blocked == "https://h0.example");
        assert_eq!(h0.map(|e| e.count), Some(2));
    }

    #[test]
    fn a_full_beat_stays_inside_the_report_limit() {
        let mut t = Tally::default();
        let long = "x".repeat(300);
        for i in 0..CSP_DISTINCT_MAX {
            let r = Report::new(
                "script-src-elem",
                &format!("{i}{long}"),
                &format!("https://{i}{long}"),
                &format!("https://h/{long}"),
                "android-webview",
            );
            t.record(r, i64::MAX);
        }
        let json = serde_json::to_vec(&mailrs_ops_report::wire::Detail::Webapi(t.beat())).unwrap();
        let limit = mailrs_ops_report::wire::MAX_REPORT_BYTES - 4 * 1024;
        assert!(json.len() < limit, "{}", json.len());
        assert!(t.dropped > 0);
    }

    #[test]
    fn clients_are_classed_by_their_user_agent() {
        let android_wv = "Mozilla/5.0 (Linux; Android 14; Pixel 8 Build/AP1A; wv) AppleWebKit/537.36 Chrome/120 Mobile Safari/537.36";
        let iphone = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 Mobile/15E148";
        let mac =
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/605.1.15 Safari/605.1.15";
        assert_eq!(client_class(Some(android_wv)), "android-webview");
        assert_eq!(client_class(Some(iphone)), "mobile");
        assert_eq!(client_class(Some(mac)), "desktop");
        assert_eq!(client_class(Some("")), "unknown");
        assert_eq!(client_class(None), "unknown");
    }

    #[test]
    fn only_the_first_path_segment_of_the_page_is_kept() {
        assert_eq!(first_segment("https://h/mail/thread/42?t=x"), "mail");
        assert_eq!(first_segment("https://h/settings#a"), "settings");
        assert_eq!(first_segment("https://h/"), "");
        assert_eq!(first_segment("https://h"), "");
        assert_eq!(first_segment("https://h?token=1"), "");
        assert_eq!(first_segment(""), "");
    }

    #[test]
    fn a_directive_no_policy_has_is_other() {
        assert_eq!(directive_name("script-src-elem"), "script-src-elem");
        assert_eq!(directive_name("img-src 'self' data:"), "img-src");
        assert_eq!(directive_name("made-up-thing"), "other");
        assert_eq!(directive_name(""), "other");
    }
}
