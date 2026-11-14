//! Network-kevy backend for [`mailrs_alias_store::AliasStore`].
//!
//! Each call opens a fresh `kevy_client::Connection` to the URL — same
//! per-call pattern fastcore's side-state / bounce / tlsrpt already use
//! (they can't share `&mut Connection` across async tasks). Loopback
//! open is ~250µs; alias resolve fires once per accepted SMTP message
//! so the cost is negligible next to receiver work.
//!
//! Keyspace matches the embedded backend byte-for-byte:
//! `mailrs:alias:v2:<address>` hash `{target, domain, created_at,
//! active}`. The set `mailrs:alias:sources` lists them, written in the same
//! MULTI as each alias, so every command here is one Redis and Valkey have.

use std::io;

use mailrs_alias_store::AliasStore;

const MAX_HOPS: usize = 4;

use mailrs_alias_store::keys::{ALIAS_SOURCES_KEY, ALIAS_V2_PREFIX, alias_v2_key as alias_key_v2};

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// AliasStore against a shared network kevy. Cheap to construct; the URL
/// is kept as-is and dialed per call.
pub struct NetworkKevyAliasStore {
    url: String,
}

impl NetworkKevyAliasStore {
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }

    fn connect(&self) -> kevy_client::KevyResult<kevy_client::Connection> {
        // kevy 4.0 renamed resource-opening to `connect` for network
        // handles (`open` is now reserved for local resources).
        kevy_client::Connection::connect(&self.url)
    }

    /// Bring the source set in line with the alias hashes: add the
    /// sources it is missing, drop the ones whose alias is gone. Run at
    /// boot; it also covers an alias written straight into the store
    /// (`kevy-cli`) rather than through [`AliasStore::upsert`]. A set that
    /// already matches is read and nothing is written. Returns
    /// `(added, dropped)`.
    pub fn reconcile_sources(&self) -> io::Result<(usize, usize)> {
        let mut conn = self.connect()?;
        let mut pattern = ALIAS_V2_PREFIX.to_vec();
        pattern.push(b'*');
        let mut held = std::collections::BTreeSet::new();
        let mut cursor = 0;
        loop {
            let (next, batch) = conn.scan(cursor, Some(&pattern), Some(1000))?;
            for key in batch {
                if let Some(source) = key.strip_prefix(ALIAS_V2_PREFIX) {
                    held.insert(source.to_vec());
                }
            }
            if next == 0 {
                break;
            }
            cursor = next;
        }
        let listed: std::collections::BTreeSet<Vec<u8>> =
            conn.smembers(ALIAS_SOURCES_KEY)?.into_iter().collect();
        let missing: Vec<&[u8]> = held.difference(&listed).map(Vec::as_slice).collect();
        let stale: Vec<&[u8]> = listed.difference(&held).map(Vec::as_slice).collect();
        if !missing.is_empty() {
            conn.sadd(ALIAS_SOURCES_KEY, &missing)?;
        }
        if !stale.is_empty() {
            conn.srem(ALIAS_SOURCES_KEY, &stale)?;
        }
        Ok((missing.len(), stale.len()))
    }
}

impl AliasStore for NetworkKevyAliasStore {
    fn resolve(&self, address: &str) -> io::Result<Option<String>> {
        let mut conn = self.connect()?;
        let mut current = address.to_string();
        let mut hit_any = false;
        for hop in 0..MAX_HOPS {
            let key_v2 = alias_key_v2(&current);
            let mut target = conn.hget(key_v2.as_bytes(), b"target")?;
            // Domain catch-all — an entry keyed `@example.com` answers
            // for every local part in that domain with no explicit alias
            // and no mailbox. Mirrors `KevyMailboxStore::resolve_alias`;
            // this is the impl fastcore actually runs in prod, and the
            // embedded one only serves tests, so a catch-all added to
            // just one of them does nothing where it matters
            // (2026-07-20).
            //
            // First hop only: a catch-all is inbound policy, not a link
            // in a chain, and firing it mid-chain would silently reroute
            // a deliberate alias whose target is merely wrong.
            if target.is_none()
                && hop == 0
                && let Some((_, domain)) = current.rsplit_once('@')
            {
                let key = alias_key_v2(&format!("@{domain}"));
                target = conn.hget(key.as_bytes(), b"target")?;
            }
            let Some(raw) = target else {
                return Ok(if hit_any { Some(current) } else { None });
            };
            let Ok(next) = String::from_utf8(raw) else {
                return Ok(if hit_any { Some(current) } else { None });
            };
            if next.is_empty() || next == current {
                return Ok(if hit_any { Some(current) } else { None });
            }
            hit_any = true;
            current = next;
        }
        Ok(Some(current))
    }

    fn upsert(&self, source: &str, target: &str) -> io::Result<()> {
        let mut conn = self.connect()?;
        let key_v2 = alias_key_v2(source);
        let domain = source.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
        let created_at = now_secs().to_string();
        let mut tx = conn.multi()?;
        tx.queue(&[
            b"HSET",
            key_v2.as_bytes(),
            b"target",
            target.as_bytes(),
            b"domain",
            domain.as_bytes(),
            b"created_at",
            created_at.as_bytes(),
            b"active",
            b"1",
        ])?;
        tx.queue(&[b"SADD", ALIAS_SOURCES_KEY, source.as_bytes()])?;
        tx.exec()?;
        Ok(())
    }

    fn delete(&self, source: &str) -> io::Result<bool> {
        let mut conn = self.connect()?;
        let key_v2 = alias_key_v2(source);
        let mut tx = conn.multi()?;
        tx.queue(&[b"DEL", key_v2.as_bytes()])?;
        tx.queue(&[b"SREM", ALIAS_SOURCES_KEY, source.as_bytes()])?;
        let replies = tx.exec()?;
        Ok(matches!(replies.first(), Some(kevy_client::Reply::Int(n)) if *n > 0))
    }

    /// Two round trips: the source set, then one pipelined `HGET target`
    /// per alias.
    fn list(&self) -> io::Result<Vec<(String, String)>> {
        let mut conn = self.connect()?;
        let keys: Vec<Vec<u8>> = conn
            .smembers(ALIAS_SOURCES_KEY)?
            .into_iter()
            .map(|source| [ALIAS_V2_PREFIX, &source].concat())
            .collect();
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let replies = conn.pipeline(|p| {
            for key in &keys {
                p.cmd(&[b"HGET", key, b"target"]);
            }
        })?;
        let mut out = Vec::with_capacity(keys.len());
        for (i, reply) in replies.into_iter().enumerate() {
            let Some(source_bytes) = keys[i].strip_prefix(ALIAS_V2_PREFIX) else {
                continue;
            };
            let Ok(source) = std::str::from_utf8(source_bytes) else {
                continue;
            };
            let target = match reply {
                kevy_client::Reply::Bulk(b) => match String::from_utf8(b) {
                    Ok(s) => s,
                    Err(_) => continue,
                },
                _ => continue,
            };
            out.push((source.to_string(), target));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The store production runs, against a real server: kevy-server, or
    /// Redis or Valkey when `MAILRS_TEST_NETWORK_STORE` says so.
    async fn on_a_server(check: impl FnOnce(NetworkKevyAliasStore) + Send + 'static) {
        let (_server, url) = mailrs_test_docker::kevy::kevy_server().await;
        tokio::task::spawn_blocking(move || {
            let store = NetworkKevyAliasStore::new(url);
            // as fastcore does at boot
            store.reconcile_sources().expect("sources");
            check(store)
        })
        .await
        .expect("check");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn honours_the_shared_catch_all_contract() {
        on_a_server(|store| mailrs_alias_store::assert_catch_all_contract(&store)).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_alias_resolves_lists_and_goes() {
        on_a_server(|store| {
            let src = "test-net-alias@example.com";
            let tgt = "recipient@example.com";
            store.upsert(src, tgt).unwrap();
            assert_eq!(store.resolve(src).unwrap().as_deref(), Some(tgt));
            // case is kept as given, as on the embedded store
            let upper = "Test-Net-Alias@example.com";
            store.upsert(upper, tgt).unwrap();
            assert_eq!(store.resolve(upper).unwrap().as_deref(), Some(tgt));
            let mut listed = store.list().unwrap();
            listed.sort();
            assert_eq!(
                listed,
                vec![
                    (upper.to_string(), tgt.to_string()),
                    (src.to_string(), tgt.to_string()),
                ]
            );
            assert!(store.delete(src).unwrap());
            assert!(store.delete(upper).unwrap());
            assert!(store.list().unwrap().is_empty());
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn boot_lists_what_was_written_past_the_store() {
        on_a_server(|store| {
            let mut conn = store.connect().unwrap();
            // an alias written straight into the store, as `kevy-cli` would
            conn.hset(
                alias_key_v2("cli@example.com").as_bytes(),
                &[(b"target".as_slice(), b"to@example.com".as_slice())],
            )
            .unwrap();
            // and a listed source whose alias is gone
            conn.sadd(ALIAS_SOURCES_KEY, &[b"gone@example.com".as_slice()])
                .unwrap();
            assert_eq!(store.reconcile_sources().unwrap(), (1, 1));
            assert_eq!(
                store.list().unwrap(),
                vec![("cli@example.com".to_string(), "to@example.com".to_string())]
            );
            assert_eq!(
                store.reconcile_sources().unwrap(),
                (0, 0),
                "in line already"
            );
        })
        .await;
    }
}
