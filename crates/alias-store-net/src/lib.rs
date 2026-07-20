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
//! active}`; range-indexed by `aliases_by_domain` /
//! `aliases_by_target` for `list()`'s 2-RTT walk.

use std::io;

use mailrs_alias_store::AliasStore;

const MAX_HOPS: usize = 4;

/// v2.6.0 §P6 dual-write: parallel hash keyspace for the alias table.
/// Mirrors `mailrs_mailbox_kevy::keys::alias_v2`.
fn alias_key_v2(address: &str) -> String {
    format!("mailrs:alias:v2:{address}")
}
const ALIAS_V2_PREFIX: &[u8] = b"mailrs:alias:v2:";
const IDX_ALIASES_BY_DOMAIN: &[u8] = b"aliases_by_domain";
const IDX_ALIASES_BY_TARGET: &[u8] = b"aliases_by_target";

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

    fn connect(&self) -> io::Result<kevy_client::Connection> {
        kevy_client::Connection::open(&self.url)
    }

    /// v2.6.0 §P6 dual-write: idempotently declare the alias-side range
    /// indexes on the network kevy. Callers invoke once at startup;
    /// duplicate declarations on the server return an error which we
    /// swallow (the catalog persists the spec on first call).
    pub fn ensure_indexes(&self) -> io::Result<()> {
        let mut conn = self.connect()?;
        let _ = conn.idx_create_range(
            IDX_ALIASES_BY_DOMAIN,
            ALIAS_V2_PREFIX,
            b"domain",
            kevy_client::IdxType::Str,
        );
        let _ = conn.idx_create_range(
            IDX_ALIASES_BY_TARGET,
            ALIAS_V2_PREFIX,
            b"target",
            kevy_client::IdxType::Str,
        );
        Ok(())
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
        conn.hset(
            key_v2.as_bytes(),
            &[
                (b"target".as_slice(), target.as_bytes()),
                (b"domain".as_slice(), domain.as_bytes()),
                (b"created_at".as_slice(), created_at.as_bytes()),
                (b"active".as_slice(), b"1".as_slice()),
            ],
        )?;
        Ok(())
    }

    fn delete(&self, source: &str) -> io::Result<bool> {
        let mut conn = self.connect()?;
        let key_v2 = alias_key_v2(source);
        let removed = conn.del(&[key_v2.as_bytes()])?;
        Ok(removed > 0)
    }

    /// v2.6.1 §P6 read cutover: 2-RTT list via `IDX.QUERY RANGE` on
    /// `aliases_by_domain` + a single pipelined `HGET target` batch.
    /// Replaces the pre-Phase-10 `SMEMBERS + N × GET` fanout
    /// (41 RTT for the 40-alias prod dataset → 2 RTT).
    ///
    /// The index is sorted by `(domain, key)` — same result set as
    /// the legacy path, just server-sorted. The empty min / `\xff\xff`
    /// max span every Str value. `IdxRow.key` is the full kevy key
    /// including the `mailrs:alias:v2:` prefix which we strip to
    /// recover the source address.
    fn list(&self) -> io::Result<Vec<(String, String)>> {
        let mut conn = self.connect()?;
        let mut keys: Vec<Vec<u8>> = Vec::new();
        let mut cursor: Option<Vec<u8>> = None;
        loop {
            let page = conn.idx_query_range(
                IDX_ALIASES_BY_DOMAIN,
                b"",
                b"\xff\xff",
                10_000,
                cursor.as_deref(),
            )?;
            for row in page.rows {
                keys.push(row.key);
            }
            match page.cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
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

    /// Opt-in integration test — set `MAILRS_TEST_KEVY_URL` to a live
    /// kevy-server URL (e.g. `kevy://127.0.0.1:6379`) to exercise the
    /// full RESP roundtrip. CI leaves it unset so the workspace test
    /// pass stays hermetic; the store's contract shape is already
    /// covered by `mailrs_alias_store::MemoryAliasStore` and the
    /// embedded-kevy trait impl tests.
    /// Opt-in like the roundtrip below — the catch-all contract cannot
    /// be checked hermetically here, but this is the impl prod runs, so
    /// it is the one that most needs checking.
    #[test]
    fn honours_the_shared_catch_all_contract_when_kevy_url_set() {
        let Ok(url) = std::env::var("MAILRS_TEST_KEVY_URL") else {
            eprintln!("skipping: MAILRS_TEST_KEVY_URL not set");
            return;
        };
        let store = NetworkKevyAliasStore::new(url);
        mailrs_alias_store::assert_catch_all_contract(&store);
    }

    #[test]
    fn network_roundtrip_when_kevy_url_set() {
        let Ok(url) = std::env::var("MAILRS_TEST_KEVY_URL") else {
            eprintln!("skipping: MAILRS_TEST_KEVY_URL not set");
            return;
        };
        let store = NetworkKevyAliasStore::new(url);
        let src = "test-net-alias@example.com";
        let tgt = "recipient@example.com";
        // Clean slate — the shared kevy may have leftovers from prior runs.
        store.delete(src).unwrap();
        store.upsert(src, tgt).unwrap();
        assert_eq!(store.resolve(src).unwrap().as_deref(), Some(tgt));
        // Case-normalization regression from the embed backend applies here too.
        let upper = "Test-Net-Alias@example.com";
        store.delete(upper).unwrap();
        store.upsert(upper, tgt).unwrap();
        assert_eq!(store.resolve(upper).unwrap().as_deref(), Some(tgt));
        // Cleanup.
        assert!(store.delete(src).unwrap());
        assert!(store.delete(upper).unwrap());
    }
}
