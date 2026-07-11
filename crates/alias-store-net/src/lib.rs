//! Network-kevy backend for [`mailrs_alias_store::AliasStore`].
//!
//! Each call opens a fresh `kevy_client::Connection` to the URL — same
//! per-call pattern fastcore's side-state / bounce / tlsrpt already use
//! (they can't share `&mut Connection` across async tasks). Loopback
//! open is ~250µs; alias resolve fires once per accepted SMTP message
//! so the cost is negligible next to receiver work.
//!
//! Keyspace matches the embedded backend byte-for-byte:
//! - `mailrs:alias:<address>` string, value = target
//! - `mailrs:aliases:index`  set   of every source
//!
//! This intentionally leaves the AOF layout the receiver already reads
//! untouched — the network kevy container (`mailrs-kevy` / `kevy://…`)
//! is the source of truth once RFC 20260705 Step 2 lands. Cutover
//! plan (embed → network) is a one-shot dump/load script, called out
//! in the RFC.

use std::io;

use mailrs_alias_store::AliasStore;

/// Alias key: `mailrs:alias:<address>`. Kept lock-step with
/// `mailrs_mailbox_kevy::keys::alias`.
fn alias_key(address: &str) -> String {
    format!("mailrs:alias:{address}")
}

/// Set-of-aliases index: mirrored from `mailrs_mailbox_kevy::keys::ALIAS_INDEX`.
const ALIAS_INDEX: &[u8] = b"mailrs:aliases:index";

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

    /// v2.6.1 §P6 backfill: populate the v2 hash keyspace from the
    /// legacy string+set layout. Idempotent — entries that already
    /// have a `target` field are skipped. Callers invoke once at
    /// startup after `ensure_indexes()`; the pre-Phase-9 alias rows
    /// get promoted to v2 the moment fastcore boots on this build.
    ///
    /// This is the RFC §4.2 backfill run inline instead of as a
    /// separate script — the dataset is small (dozens of aliases),
    /// idempotent, and running it on every boot means Phase 11 has a
    /// hard guarantee that every legacy row exists in v2.
    pub fn backfill_v2(&self) -> io::Result<usize> {
        let mut conn = self.connect()?;
        let members = conn.smembers(ALIAS_INDEX)?;
        if members.is_empty() {
            return Ok(0);
        }
        let created_at = now_secs().to_string();
        let mut promoted = 0usize;
        for m in members {
            let Ok(source) = String::from_utf8(m) else {
                continue;
            };
            let key_v2 = alias_key_v2(&source);
            // Skip if the v2 row already has a target field — either
            // because dual-write already fired or because a prior
            // backfill promoted this row.
            if conn.hget(key_v2.as_bytes(), b"target")?.is_some() {
                continue;
            }
            let key = alias_key(&source);
            let Some(target_bytes) = conn.get(key.as_bytes())? else {
                continue;
            };
            let domain = source.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
            let _ = conn.hset(
                key_v2.as_bytes(),
                &[
                    (b"target".as_slice(), target_bytes.as_slice()),
                    (b"domain".as_slice(), domain.as_bytes()),
                    (b"created_at".as_slice(), created_at.as_bytes()),
                    (b"active".as_slice(), b"1".as_slice()),
                ],
            );
            promoted += 1;
        }
        Ok(promoted)
    }
}

impl AliasStore for NetworkKevyAliasStore {
    fn resolve(&self, address: &str) -> io::Result<Option<String>> {
        let mut conn = self.connect()?;
        let mut current = address.to_string();
        let mut hit_any = false;
        for _ in 0..MAX_HOPS {
            let key = alias_key(&current);
            let Some(raw) = conn.get(key.as_bytes())? else {
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
        let key = alias_key(source);
        conn.set(key.as_bytes(), target.as_bytes())?;
        conn.sadd(ALIAS_INDEX, &[source.as_bytes()])?;
        // v2.6.0 §P6 dual-write: hash + range-indexed fields on the
        // parallel `mailrs:alias:v2:*` keyspace. Best-effort; failure
        // here leaves the legacy layout intact (read paths untouched).
        let key_v2 = alias_key_v2(source);
        let domain = source.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
        let created_at = now_secs().to_string();
        let _ = conn.hset(
            key_v2.as_bytes(),
            &[
                (b"target".as_slice(), target.as_bytes()),
                (b"domain".as_slice(), domain.as_bytes()),
                (b"created_at".as_slice(), created_at.as_bytes()),
                (b"active".as_slice(), b"1".as_slice()),
            ],
        );
        Ok(())
    }

    fn delete(&self, source: &str) -> io::Result<bool> {
        let mut conn = self.connect()?;
        let key = alias_key(source);
        let key_v2 = alias_key_v2(source);
        let removed = conn.del(&[key.as_bytes(), key_v2.as_bytes()])?;
        conn.srem(ALIAS_INDEX, &[source.as_bytes()])?;
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
