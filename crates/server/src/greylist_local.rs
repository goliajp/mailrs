//! Local PG-backed greylist white/black lists (Phase 2).
//!
//! Layers operator-controlled per-server lists on top of Phase 1's remote
//! sender-domain whitelist. Pipeline precedence (see
//! `inbound::stages::greylist`):
//!
//! ```text
//! local-black hit  → Reject 550 5.7.1
//! local-white hit  → skip greylist (skip triplet + skip remote whitelist)
//! remote-white hit → skip greylist (Phase 1)
//! triplet check    → Defer / TooEarly / Accept
//! ```
//!
//! Schema mutex `UNIQUE (kind, value)` (no `list`) prevents the same key
//! from being on both lists at the database layer; black-before-white code
//! ordering is a belt-and-suspenders.

use std::collections::HashSet;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use ipnet::IpNet;
use tokio::sync::RwLock;

/// In-memory snapshot. Built once at boot and atomically swapped on every
/// reload. The struct is `Clone`-friendly via `Arc<RwLock<...>>` only — the
/// inner snapshot itself never mutates after construction.
#[derive(Default, Debug)]
pub struct GreylistLocalLists {
    pub white_domains: HashSet<String>,
    pub white_emails: HashSet<String>,
    pub white_cidrs: Vec<IpNet>,
    pub black_domains: HashSet<String>,
    pub black_emails: HashSet<String>,
    pub black_cidrs: Vec<IpNet>,
    pub last_reload_at: Option<u64>,
    pub last_error: Option<String>,
}

impl GreylistLocalLists {
    /// Total entry count (sum of all six sets).
    pub fn total(&self) -> usize {
        self.white_domains.len()
            + self.white_emails.len()
            + self.white_cidrs.len()
            + self.black_domains.len()
            + self.black_emails.len()
            + self.black_cidrs.len()
    }

    /// White-list entry count.
    pub fn white_count(&self) -> usize {
        self.white_domains.len() + self.white_emails.len() + self.white_cidrs.len()
    }

    /// Black-list entry count.
    pub fn black_count(&self) -> usize {
        self.black_domains.len() + self.black_emails.len() + self.black_cidrs.len()
    }

    /// Check whether the (sender, client_ip) tuple matches any black-list
    /// entry. Returns `Some(kind)` for metrics; `None` on no match.
    ///
    /// Match contract:
    /// - email match is case-insensitive (stored lowercased)
    /// - domain match uses ancestor walk (`mail.gmail.com` → `gmail.com`)
    /// - cidr match accepts both v4 and v6 sender IPs
    /// - empty sender (bounce `<>`) or missing `@` → only CIDR check applies
    pub fn matches_black(&self, sender: &str, client_ip: IpAddr) -> Option<&'static str> {
        match_any(
            sender,
            client_ip,
            &self.black_emails,
            &self.black_domains,
            &self.black_cidrs,
        )
    }

    /// Same as [`matches_black`](Self::matches_black) but against the
    /// white-list sets.
    pub fn matches_white(&self, sender: &str, client_ip: IpAddr) -> Option<&'static str> {
        match_any(
            sender,
            client_ip,
            &self.white_emails,
            &self.white_domains,
            &self.white_cidrs,
        )
    }
}

fn match_any(
    sender: &str,
    client_ip: IpAddr,
    emails: &HashSet<String>,
    domains: &HashSet<String>,
    cidrs: &[IpNet],
) -> Option<&'static str> {
    let lower = sender.to_lowercase();
    if !emails.is_empty() && !lower.is_empty() && emails.contains(&lower) {
        return Some("email");
    }
    if !domains.is_empty()
        && let Some((_, domain)) = lower.rsplit_once('@')
        && !domain.is_empty()
    {
        if domains.contains(domain) {
            return Some("domain");
        }
        let mut rest = domain;
        while let Some(idx) = rest.find('.') {
            rest = &rest[idx + 1..];
            if domains.contains(rest) {
                return Some("domain");
            }
        }
    }
    if !cidrs.is_empty() && cidrs.iter().any(|n| n.contains(&client_ip)) {
        return Some("cidr");
    }
    None
}

/// Shareable handle for the live snapshot.
pub type GreylistLocalHandle = Arc<RwLock<GreylistLocalLists>>;

/// Construct an empty handle (no PG configured, or PG unavailable at boot).
pub fn empty() -> GreylistLocalHandle {
    Arc::new(RwLock::new(GreylistLocalLists::default()))
}

/// Query PG for all rows, build a fresh snapshot, and atomically install
/// it. Errors are recorded on the handle (`last_error`) but never panic.
pub async fn reload(handle: &GreylistLocalHandle, pool: &sqlx::PgPool) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    match load_from_pg(pool).await {
        Ok(mut snapshot) => {
            snapshot.last_reload_at = Some(now);
            snapshot.last_error = None;
            let total = snapshot.total();
            let white = snapshot.white_count();
            let black = snapshot.black_count();
            *handle.write().await = snapshot;
            tracing::debug!(
                target: "greylist.local",
                total,
                white,
                black,
                "greylist_local snapshot reloaded"
            );
            metrics::counter!("mailrs_greylist_local_reload_total", "outcome" => "ok").increment(1);
            metrics::gauge!("mailrs_greylist_local_size", "list" => "white", "kind" => "any")
                .set(white as f64);
            metrics::gauge!("mailrs_greylist_local_size", "list" => "black", "kind" => "any")
                .set(black as f64);
        }
        Err(e) => {
            let msg = e.to_string();
            tracing::warn!(
                target: "greylist.local",
                error = %msg,
                "greylist_local reload failed; keeping previous snapshot"
            );
            let mut g = handle.write().await;
            g.last_error = Some(msg);
            metrics::counter!("mailrs_greylist_local_reload_total", "outcome" => "error")
                .increment(1);
        }
    }
}

async fn load_from_pg(pool: &sqlx::PgPool) -> Result<GreylistLocalLists, sqlx::Error> {
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT kind, list, value FROM greylist_local_lists")
            .fetch_all(pool)
            .await?;

    let mut s = GreylistLocalLists::default();
    for (kind, list, value) in rows {
        let is_black = list == "black";
        match kind.as_str() {
            "domain" => {
                let v = value.to_lowercase();
                if is_black {
                    s.black_domains.insert(v);
                } else {
                    s.white_domains.insert(v);
                }
            }
            "email" => {
                let v = value.to_lowercase();
                if is_black {
                    s.black_emails.insert(v);
                } else {
                    s.white_emails.insert(v);
                }
            }
            "cidr" => {
                if let Ok(net) = IpNet::from_str(&value) {
                    if is_black {
                        s.black_cidrs.push(net);
                    } else {
                        s.white_cidrs.push(net);
                    }
                } else {
                    tracing::warn!(
                        target: "greylist.local",
                        value = %value,
                        "skipping unparseable cidr row"
                    );
                }
            }
            other => {
                tracing::warn!(
                    target: "greylist.local",
                    kind = %other,
                    "skipping row with unknown kind"
                );
            }
        }
    }
    Ok(s)
}

/// Spawn a background task that periodically refreshes the snapshot.
///
/// Cadence is `interval_secs`. First reload runs immediately so the
/// snapshot is populated before any inbound mail is accepted (the boot
/// path also calls `reload` synchronously for the same reason — this task
/// is the periodic refresher, not the boot loader).
pub fn spawn_reload_task(
    handle: GreylistLocalHandle,
    pool: sqlx::PgPool,
    interval_secs: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(interval_secs));
        // first tick fires instantly — we already reloaded at boot, skip it
        tick.tick().await;
        loop {
            tick.tick().await;
            reload(&handle, &pool).await;
        }
    })
}

// ---------------------------------------------------------------------
// Value normalization + validation. Used by the admin API to canonicalize
// user input before INSERT. The public ValueError is mapped to HTTP 400 /
// MCP InvalidParam by the handler layer.
// ---------------------------------------------------------------------

#[derive(Debug)]
pub enum ValueError {
    InvalidKind,
    InvalidList,
    Empty,
    BadDomain(String),
    BadEmail(String),
    BadCidr(String),
    CidrNotCanonical { raw: String, canonical: String },
}

impl std::fmt::Display for ValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKind => f.write_str("kind must be one of: domain, email, cidr"),
            Self::InvalidList => f.write_str("list must be one of: white, black"),
            Self::Empty => f.write_str("value must not be empty"),
            Self::BadDomain(v) => write!(f, "value '{v}' is not a valid domain"),
            Self::BadEmail(v) => write!(f, "value '{v}' is not a valid email (need user@host)"),
            Self::BadCidr(v) => write!(f, "value '{v}' is not a valid CIDR"),
            Self::CidrNotCanonical { raw, canonical } => {
                write!(f, "value '{raw}' is not canonical — use {canonical}")
            }
        }
    }
}

impl std::error::Error for ValueError {}

/// Normalize a (kind, value) pair: lowercases domain/email, parses CIDR
/// to canonical form, and rejects malformed inputs with a precise error.
pub fn normalize(kind: &str, value: &str) -> Result<String, ValueError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ValueError::Empty);
    }
    match kind {
        "domain" => {
            let d = trimmed.trim_matches('.').to_lowercase();
            if d.is_empty() || !d.contains('.') || d.contains(' ') || d.contains('@') {
                return Err(ValueError::BadDomain(value.to_string()));
            }
            Ok(d)
        }
        "email" => {
            let e = trimmed.to_lowercase();
            let Some((local, domain)) = e.rsplit_once('@') else {
                return Err(ValueError::BadEmail(value.to_string()));
            };
            if local.is_empty() || domain.is_empty() || !domain.contains('.') {
                return Err(ValueError::BadEmail(value.to_string()));
            }
            Ok(e)
        }
        "cidr" => {
            let net =
                IpNet::from_str(trimmed).map_err(|_| ValueError::BadCidr(value.to_string()))?;
            let canonical = net.trunc().to_string();
            if canonical != trimmed {
                return Err(ValueError::CidrNotCanonical {
                    raw: value.to_string(),
                    canonical,
                });
            }
            Ok(canonical)
        }
        _ => Err(ValueError::InvalidKind),
    }
}

/// Validate the `list` discriminator.
pub fn validate_list(list: &str) -> Result<(), ValueError> {
    match list {
        "white" | "black" => Ok(()),
        _ => Err(ValueError::InvalidList),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn matches_black_email_direct() {
        let mut s = GreylistLocalLists::default();
        s.black_emails.insert("bob@example.com".into());
        assert_eq!(
            s.matches_black("bob@example.com", ip("1.1.1.1")),
            Some("email")
        );
        assert_eq!(
            s.matches_black("BOB@EXAMPLE.COM", ip("1.1.1.1")),
            Some("email")
        );
        assert!(
            s.matches_black("alice@example.com", ip("1.1.1.1"))
                .is_none()
        );
    }

    #[test]
    fn matches_black_domain_ancestor() {
        let mut s = GreylistLocalLists::default();
        s.black_domains.insert("example.com".into());
        assert_eq!(
            s.matches_black("a@example.com", ip("1.1.1.1")),
            Some("domain")
        );
        assert_eq!(
            s.matches_black("a@mail.example.com", ip("1.1.1.1")),
            Some("domain")
        );
        assert_eq!(
            s.matches_black("A@MAIL.EXAMPLE.COM", ip("1.1.1.1")),
            Some("domain")
        );
        assert!(s.matches_black("a@notexample.com", ip("1.1.1.1")).is_none());
    }

    #[test]
    fn matches_black_cidr_v4() {
        let mut s = GreylistLocalLists::default();
        s.black_cidrs.push(IpNet::from_str("10.0.0.0/8").unwrap());
        assert_eq!(s.matches_black("any@host", ip("10.1.2.3")), Some("cidr"));
        assert!(s.matches_black("any@host", ip("11.1.2.3")).is_none());
    }

    #[test]
    fn matches_black_cidr_v6() {
        let mut s = GreylistLocalLists::default();
        s.black_cidrs
            .push(IpNet::from_str("2001:db8::/32").unwrap());
        assert_eq!(s.matches_black("a@b.com", ip("2001:db8::1")), Some("cidr"));
        assert!(s.matches_black("a@b.com", ip("2001:dead::1")).is_none());
    }

    #[test]
    fn empty_sender_only_cidr_matches() {
        let mut s = GreylistLocalLists::default();
        s.black_emails.insert("".into()); // pathological: empty as data
        s.black_domains.insert("example.com".into());
        s.black_cidrs.push(IpNet::from_str("10.0.0.0/8").unwrap());
        // empty sender: skip email + domain (both gated on non-empty), cidr still fires
        assert_eq!(s.matches_black("", ip("10.1.2.3")), Some("cidr"));
        assert!(s.matches_black("", ip("11.1.2.3")).is_none());
    }

    #[test]
    fn no_at_sender_only_cidr() {
        let mut s = GreylistLocalLists::default();
        s.black_domains.insert("example.com".into());
        s.black_cidrs.push(IpNet::from_str("10.0.0.0/8").unwrap());
        // no '@' in sender: domain split fails silently
        assert_eq!(s.matches_black("postmaster", ip("10.1.2.3")), Some("cidr"));
        assert!(s.matches_black("postmaster", ip("11.1.2.3")).is_none());
    }

    #[test]
    fn black_independent_of_white() {
        let mut s = GreylistLocalLists::default();
        s.white_domains.insert("example.com".into());
        s.black_emails.insert("bob@example.com".into());
        // bob@example.com hits black via email
        assert_eq!(
            s.matches_black("bob@example.com", ip("1.1.1.1")),
            Some("email")
        );
        // …but anyone-else@example.com hits white via domain
        assert!(
            s.matches_black("alice@example.com", ip("1.1.1.1"))
                .is_none()
        );
        assert_eq!(
            s.matches_white("alice@example.com", ip("1.1.1.1")),
            Some("domain")
        );
    }

    #[test]
    fn counts() {
        let mut s = GreylistLocalLists::default();
        s.white_domains.insert("a.com".into());
        s.white_emails.insert("u@a.com".into());
        s.black_cidrs.push(IpNet::from_str("10.0.0.0/8").unwrap());
        assert_eq!(s.total(), 3);
        assert_eq!(s.white_count(), 2);
        assert_eq!(s.black_count(), 1);
    }

    #[test]
    fn normalize_domain_ok() {
        assert_eq!(normalize("domain", "Gmail.COM").unwrap(), "gmail.com");
        assert_eq!(
            normalize("domain", "  example.org  ").unwrap(),
            "example.org"
        );
        assert_eq!(normalize("domain", ".example.org.").unwrap(), "example.org");
    }

    #[test]
    fn normalize_domain_rejects_garbage() {
        assert!(matches!(
            normalize("domain", "").unwrap_err(),
            ValueError::Empty
        ));
        assert!(matches!(
            normalize("domain", "nodot").unwrap_err(),
            ValueError::BadDomain(_)
        ));
        assert!(matches!(
            normalize("domain", "has space.com").unwrap_err(),
            ValueError::BadDomain(_)
        ));
        assert!(matches!(
            normalize("domain", "user@example.com").unwrap_err(),
            ValueError::BadDomain(_)
        ));
    }

    #[test]
    fn normalize_email_ok() {
        assert_eq!(
            normalize("email", "User@Example.COM").unwrap(),
            "user@example.com"
        );
    }

    #[test]
    fn normalize_email_rejects_garbage() {
        assert!(matches!(
            normalize("email", "noatsign").unwrap_err(),
            ValueError::BadEmail(_)
        ));
        assert!(matches!(
            normalize("email", "@example.com").unwrap_err(),
            ValueError::BadEmail(_)
        ));
        assert!(matches!(
            normalize("email", "user@").unwrap_err(),
            ValueError::BadEmail(_)
        ));
        assert!(matches!(
            normalize("email", "user@nodot").unwrap_err(),
            ValueError::BadEmail(_)
        ));
    }

    #[test]
    fn normalize_cidr_ok() {
        assert_eq!(normalize("cidr", "10.0.0.0/8").unwrap(), "10.0.0.0/8");
        assert_eq!(normalize("cidr", "2001:db8::/32").unwrap(), "2001:db8::/32");
    }

    #[test]
    fn normalize_cidr_rejects_non_canonical() {
        // 10.0.0.1/8 has host bits set — canonical is 10.0.0.0/8
        let err = normalize("cidr", "10.0.0.1/8").unwrap_err();
        match err {
            ValueError::CidrNotCanonical { canonical, .. } => {
                assert_eq!(canonical, "10.0.0.0/8");
            }
            other => panic!("expected CidrNotCanonical, got {other:?}"),
        }
    }

    #[test]
    fn normalize_cidr_rejects_garbage() {
        assert!(matches!(
            normalize("cidr", "not-an-ip").unwrap_err(),
            ValueError::BadCidr(_)
        ));
    }

    #[test]
    fn normalize_invalid_kind() {
        assert!(matches!(
            normalize("ipv4", "10.0.0.0/8").unwrap_err(),
            ValueError::InvalidKind
        ));
    }

    #[test]
    fn validate_list_accepts_known() {
        assert!(validate_list("white").is_ok());
        assert!(validate_list("black").is_ok());
    }

    #[test]
    fn validate_list_rejects_unknown() {
        assert!(matches!(
            validate_list("grey").unwrap_err(),
            ValueError::InvalidList
        ));
        assert!(matches!(
            validate_list("").unwrap_err(),
            ValueError::InvalidList
        ));
    }
}
