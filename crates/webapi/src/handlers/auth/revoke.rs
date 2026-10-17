//! Ending every credential an address holds.
//!
//! Two kinds live in the network kevy: browser sessions (`session:<token>`,
//! indexed per address by `session:by_addr:<address>`, which `issue_session`
//! writes) and agent keys (`agent:keys:<address>` holds the records,
//! `agent:key:secret:<key>` maps a key back to `{user, id}`). The auth
//! middleware accepts an agent key only while its record is still in the
//! owner's hash, so dropping the hash is what revokes; the secret indexes
//! are dropped too so nothing points at a record that is gone.

use axum::http::StatusCode;
use kevy_client::Connection;

use crate::handlers::kevy_util::with_kevy;

/// Record `token` in `address`'s session index, so a password change or
/// account deletion can end it, and drop the tokens whose session has
/// expired: the index has no TTL of its own and would otherwise grow by
/// one entry per login forever.
pub fn index_session(c: &mut Connection, address: &str, token: &str) -> std::io::Result<()> {
    let idx = format!("session:by_addr:{address}");
    for t in c.smembers(idx.as_bytes())? {
        let key = format!("session:{}", String::from_utf8_lossy(&t));
        if c.exists(&[key.as_bytes()])? == 0 {
            c.srem(idx.as_bytes(), &[t.as_slice()])?;
        }
    }
    c.sadd(idx.as_bytes(), &[token.as_bytes()])?;
    Ok(())
}

/// Delete every session `address` holds. Returns how many tokens the
/// per-address index listed.
pub fn revoke_sessions(c: &mut Connection, address: &str) -> std::io::Result<usize> {
    let idx = format!("session:by_addr:{address}");
    let tokens = c.smembers(idx.as_bytes())?;
    for t in &tokens {
        let key = format!("session:{}", String::from_utf8_lossy(t));
        c.del(&[key.as_bytes()])?;
    }
    c.del(&[idx.as_bytes()])?;
    Ok(tokens.len())
}

/// Delete every agent key `address` owns. Returns how many secret indexes
/// pointed at it. The id counter stays, so a later account with the same
/// address never reissues an id an old index could name.
pub fn revoke_agent_keys(c: &mut Connection, address: &str) -> std::io::Result<usize> {
    c.del(&[format!("agent:keys:{address}").as_bytes()])?;
    // the index stores no owner prefix in its key, so finding an owner's
    // entries means reading each one; there are a handful per server
    let mut dropped = 0;
    for idx_key in c.keys(b"agent:key:secret:*")? {
        let Some(raw) = c.get(&idx_key)? else {
            continue;
        };
        let owner = serde_json::from_slice::<serde_json::Value>(&raw)
            .ok()
            .and_then(|v| v.get("user").and_then(|u| u.as_str()).map(String::from));
        if owner.as_deref() == Some(address) {
            c.del(&[idx_key.as_slice()])?;
            dropped += 1;
        }
    }
    Ok(dropped)
}

/// Every session and agent key of `address`, ended. What deleting an
/// account does to the credentials it left behind.
pub fn revoke_all_access(address: &str) -> Result<(), StatusCode> {
    let addr = address.to_string();
    let (sessions, keys) =
        with_kevy(move |c| Ok((revoke_sessions(c, &addr)?, revoke_agent_keys(c, &addr)?)))?;
    tracing::info!(%address, sessions, agent_keys = keys, "revoked all access");
    Ok(())
}
