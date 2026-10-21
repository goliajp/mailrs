//! Where a recipient's mail goes, worked out without writing anything.
//!
//! The same order the kevy core routes in (`fastcore/src/spool_drain/route.rs`),
//! so that pointing the web tier and the receiver at this core changes
//! nothing about who gets mail: the recipient's own mailbox; an SRS address
//! we wrote, for its original sender; an email group; an alias whose target
//! is a mailbox here; and postmaster or abuse on a local domain with no
//! route of its own, at the first local domain. A store that cannot be read
//! is an error, never "nobody".
//!
//! Email groups are read from the network kevy both cores share, because
//! that is where the web tier writes them.

use super::DomainStore;
use crate::pg::BackendPool;

/// Where a recipient's mail goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// The recipient's own mailbox.
    Own(String),
    /// A bounce to an SRS address we wrote: the original sender.
    Srs(String),
    /// An email group: each member.
    Group(Vec<String>),
    /// An alias, a domain catch-all or a role address: this mailbox.
    Alias(String),
    /// Nobody takes mail at this address.
    Nobody,
}

type Result<T> = std::result::Result<T, sqlx::Error>;

impl DomainStore {
    /// Route `raw`, the recipient as received. `maildir_root` lets a mailbox
    /// that exists on disk count before its account row does; `net_url` is
    /// the shared network kevy that holds email groups.
    pub async fn route_recipient(
        &self,
        raw: &str,
        maildir_root: &str,
        net_url: Option<&str>,
    ) -> Result<Route> {
        let fwd = raw.trim().to_ascii_lowercase();
        let pool = self
            .pg()
            .map_err(|e| sqlx::Error::Configuration(e.to_string().into()))?;
        if is_mailbox(pool, maildir_root, &fwd).await? {
            return Ok(Route::Own(fwd));
        }
        if let Some(original) = srs_original(raw.trim()) {
            return Ok(Route::Srs(original));
        }
        // before aliases: a domain catch-all answers for every address
        // without a mailbox and would take the group's mail
        if let Some(members) = group_members(pool, net_url, &fwd).await? {
            return Ok(Route::Group(members));
        }
        if let Some(target) = self.alias_target(pool, maildir_root, &fwd).await? {
            return Ok(Route::Alias(target));
        }
        Ok(match self.role_fallback(pool, maildir_root, &fwd).await? {
            Some(target) => Route::Alias(target),
            None => Route::Nobody,
        })
    }

    /// The mailbox an alias or the domain catch-all sends `fwd` to, when
    /// that target is a mailbox here.
    async fn alias_target(
        &self,
        pool: &BackendPool,
        maildir_root: &str,
        fwd: &str,
    ) -> Result<Option<String>> {
        let catch_all = fwd.rsplit_once('@').map(|(_, d)| format!("*@{d}"));
        for source in std::iter::once(fwd.to_string()).chain(catch_all) {
            let target = match &self.alias_store {
                Some(store) => {
                    let store = store.clone();
                    let s = source.clone();
                    tokio::task::spawn_blocking(move || store.resolve(&s))
                        .await
                        .map_err(|e| sqlx::Error::Io(std::io::Error::other(e)))?
                        .map_err(sqlx::Error::Io)?
                }
                None => sqlx::query_as::<_, (String,)>(
                    "SELECT target_address FROM aliases
                      WHERE source_address = $1 AND active = true
                      ORDER BY id LIMIT 1",
                )
                .bind(&source)
                .fetch_optional(pool)
                .await?
                .map(|(t,)| t),
            };
            if let Some(t) = target {
                let t = t.trim().to_ascii_lowercase();
                return Ok(is_mailbox(pool, maildir_root, &t).await?.then_some(t));
            }
        }
        Ok(None)
    }

    /// Every domain must take mail for postmaster (RFC 5321 §4.5.1), and
    /// abuse is where complaints are written (RFC 2142). One with no route
    /// of its own reaches the same address on the first local domain.
    async fn role_fallback(
        &self,
        pool: &BackendPool,
        maildir_root: &str,
        fwd: &str,
    ) -> Result<Option<String>> {
        let Some((local, domain)) = fwd.rsplit_once('@') else {
            return Ok(None);
        };
        if !matches!(local, "postmaster" | "abuse") {
            return Ok(None);
        }
        let domains = local_domains(pool).await?;
        let Some(primary) = domains.first().filter(|p| p.as_str() != domain) else {
            return Ok(None);
        };
        if !domains.iter().any(|d| d == domain) {
            return Ok(None);
        }
        let role = format!("{local}@{primary}");
        if is_mailbox(pool, maildir_root, &role).await? {
            return Ok(Some(role));
        }
        self.alias_target(pool, maildir_root, &role).await
    }
}

/// An active account, or a maildir already on disk for the address.
async fn is_mailbox(pool: &BackendPool, maildir_root: &str, addr: &str) -> Result<bool> {
    if let Some((local, domain)) = addr.split_once('@')
        && std::path::Path::new(maildir_root)
            .join(domain)
            .join(local)
            .join("new")
            .is_dir()
    {
        return Ok(true);
    }
    Ok(sqlx::query_as::<_, (bool,)>(
        "SELECT true FROM accounts WHERE address = $1 AND active = true",
    )
    .bind(addr)
    .fetch_optional(pool)
    .await?
    .is_some())
}

fn srs_original(raw: &str) -> Option<String> {
    if !raw.to_ascii_uppercase().starts_with("SRS0=") {
        return None;
    }
    let secret = std::env::var("MAILRS_SRS_SECRET").ok()?;
    mailrs_srs::reverse(raw, &secret, mailrs_srs::DEFAULT_TIMESTAMP_WINDOW_DAYS)
}

/// The members of the group at `fwd`. In the SQL core they are read from
/// the network kevy, where the web tier writes them: no network kevy
/// configured means no groups, one that cannot be reached is an error.
#[cfg(feature = "core-rpc")]
async fn group_members(
    _pool: &BackendPool,
    net_url: Option<&str>,
    fwd: &str,
) -> Result<Option<Vec<String>>> {
    let Some(url) = net_url.map(str::to_string) else {
        return Ok(None);
    };
    let fwd = fwd.to_string();
    tokio::task::spawn_blocking(move || {
        let mut conn = kevy_client::Connection::connect(&url).map_err(std::io::Error::other)?;
        mailrs_core_sidestate::families::email_groups::find_by_address(&mut conn, &fwd)
    })
    .await
    .map_err(|e| sqlx::Error::Io(std::io::Error::other(e)))?
    .map(|g| g.map(|g| g.members))
    .map_err(sqlx::Error::Io)
}

/// The members of the group at `fwd`, from the all-in-one server's own
/// tables, which is where that process keeps them.
#[cfg(not(feature = "core-rpc"))]
async fn group_members(
    pool: &BackendPool,
    _net_url: Option<&str>,
    fwd: &str,
) -> Result<Option<Vec<String>>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT m.member_address FROM email_group_members m
           JOIN email_groups g ON g.id = m.group_id
          WHERE g.address = $1",
    )
    .bind(fwd)
    .fetch_all(pool)
    .await?;
    Ok((!rows.is_empty()).then(|| rows.into_iter().map(|(a,)| a).collect()))
}

/// `MAILRS_LOCAL_DOMAINS` in its own order, then the domains added since,
/// the order the kevy core keeps them in.
async fn local_domains(pool: &BackendPool) -> Result<Vec<String>> {
    let mut out: Vec<String> = std::env::var("MAILRS_LOCAL_DOMAINS")
        .unwrap_or_default()
        .split(',')
        .map(|d| d.trim().to_ascii_lowercase())
        .filter(|d| !d.is_empty())
        .collect();
    let rows: Vec<(String,)> = sqlx::query_as("SELECT name FROM domains ORDER BY created_at, name")
        .fetch_all(pool)
        .await?;
    for (d,) in rows {
        let d = d.trim().to_ascii_lowercase();
        if !d.is_empty() && !out.contains(&d) {
            out.push(d);
        }
    }
    Ok(out)
}
