//! What this store knows about recipients, for the shared routing order in
//! `mailrs_intake::route` (see `crate::intake::PgDirectory`).
//!
//! Email groups are read from the network kevy both cores share when the
//! core-api is compiled in, because that is where the web tier writes them;
//! the all-in-one server keeps them in its own tables.

use mailrs_core_sidestate::families::email_groups::Group;

use super::DomainStore;
use crate::pg::BackendPool;

type Result<T> = std::result::Result<T, sqlx::Error>;

impl DomainStore {
    fn pool(&self) -> Result<&BackendPool> {
        self.pg()
            .map_err(|e| sqlx::Error::Configuration(e.to_string().into()))
    }

    /// Whether `addr` is an active account.
    pub(crate) async fn is_active_account(&self, addr: &str) -> Result<bool> {
        Ok(sqlx::query_as::<_, (bool,)>(
            "SELECT true FROM accounts WHERE address = $1 AND active = true",
        )
        .bind(addr)
        .fetch_optional(self.pool()?)
        .await?
        .is_some())
    }

    /// The target an alias or the domain catch-all sends `fwd` to, as
    /// stored.
    pub(crate) async fn alias_for(&self, fwd: &str) -> Result<Option<String>> {
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
                .fetch_optional(self.pool()?)
                .await?
                .map(|(t,)| t),
            };
            if target.is_some() {
                return Ok(target);
            }
        }
        Ok(None)
    }

    /// The domains added on the admin Domains page, oldest first.
    pub(crate) async fn domain_names(&self) -> Result<Vec<String>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM domains ORDER BY created_at, name")
                .fetch_all(self.pool()?)
                .await?;
        Ok(rows.into_iter().map(|(d,)| d).collect())
    }

    /// The group at `fwd`, from the network kevy the web tier writes groups
    /// to: no network kevy configured means no groups, one that cannot be
    /// reached is an error.
    #[cfg(feature = "core-rpc")]
    pub(crate) async fn email_group(
        &self,
        net_url: Option<&str>,
        fwd: &str,
    ) -> Result<Option<Group>> {
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
        .map_err(sqlx::Error::Io)
    }

    /// The group at `fwd`, from the all-in-one server's own tables, which
    /// is where that process keeps them.
    #[cfg(not(feature = "core-rpc"))]
    pub(crate) async fn email_group(
        &self,
        _net_url: Option<&str>,
        fwd: &str,
    ) -> Result<Option<Group>> {
        let rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT g.id, m.member_address FROM email_group_members m
               JOIN email_groups g ON g.id = m.group_id
              WHERE g.address = $1",
        )
        .bind(fwd)
        .fetch_all(self.pool()?)
        .await?;
        let Some(id) = rows.first().map(|(id, _)| *id) else {
            return Ok(None);
        };
        Ok(Some(Group {
            id,
            address: fwd.to_string(),
            members: rows.into_iter().map(|(_, m)| m).collect(),
        }))
    }
}
