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
        Ok(())
    }

    fn delete(&self, source: &str) -> io::Result<bool> {
        let mut conn = self.connect()?;
        let key = alias_key(source);
        let removed = conn.del(&[key.as_bytes()])?;
        conn.srem(ALIAS_INDEX, &[source.as_bytes()])?;
        Ok(removed > 0)
    }

    fn list(&self) -> io::Result<Vec<(String, String)>> {
        let mut conn = self.connect()?;
        let members = conn.smembers(ALIAS_INDEX)?;
        let mut out = Vec::with_capacity(members.len());
        for m in members {
            let Ok(source) = String::from_utf8(m) else {
                continue;
            };
            let key = alias_key(&source);
            let Some(raw) = conn.get(key.as_bytes())? else {
                continue;
            };
            let Ok(target) = String::from_utf8(raw) else {
                continue;
            };
            out.push((source, target));
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
