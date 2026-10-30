//! Ending every credential an address holds.
//!
//! Two kinds live in the network kevy: browser sessions (stored under the
//! hash of their token, indexed per address by `session:by_addr:<address>`,
//! which `issue_session` writes) and agent keys (`agent:keys:<address>` holds the records,
//! the secret index maps a key's hash back to `{user, id}`). The auth
//! middleware accepts an agent key only while its record is still in the
//! owner's hash, so dropping the hash is what revokes; the secret indexes
//! are dropped too so nothing points at a record that is gone.

use axum::http::StatusCode;
use kevy_client::Connection;
use mailrs_core_sidestate::keys::session::{
    by_addr_key, confirm_failures_key, confirmed_key, legacy_session_key, session_key,
    session_key_by_ref, session_ref,
};

use crate::handlers::kevy_util::with_kevy;

/// Record `token` in `address`'s session index, so a password change or
/// account deletion can end it, and drop the tokens whose session has
/// expired: the index has no TTL of its own and would otherwise grow by
/// one entry per login forever.
pub fn index_session(c: &mut Connection, address: &str, token: &str) -> std::io::Result<()> {
    let idx = by_addr_key(address);
    for entry in c.smembers(idx.as_bytes())? {
        let [stored, legacy, moved] = keys_of_entry(&entry);
        if c.exists(&[stored.as_bytes(), legacy.as_bytes(), moved.as_bytes()])? == 0 {
            c.srem(idx.as_bytes(), &[entry.as_slice()])?;
        }
    }
    c.sadd(idx.as_bytes(), &[session_ref(token).as_bytes()])?;
    Ok(())
}

/// Every place an index entry's session can be. The entry is a hash; or,
/// for a session issued before hashing, the token, whose session is under
/// the token until its first use and under the token's hash after it.
fn keys_of_entry(entry: &[u8]) -> [String; 3] {
    let entry = String::from_utf8_lossy(entry);
    [
        session_key_by_ref(&entry),
        legacy_session_key(&entry),
        session_key(&entry),
    ]
}

/// Delete every session `address` holds. Returns how many tokens the
/// per-address index listed.
pub fn revoke_sessions(c: &mut Connection, address: &str) -> std::io::Result<usize> {
    let idx = by_addr_key(address);
    let entries = c.smembers(idx.as_bytes())?;
    for entry in &entries {
        let [stored, legacy, moved] = keys_of_entry(entry);
        c.del(&[stored.as_bytes(), legacy.as_bytes(), moved.as_bytes()])?;
        // a confirmation outlives nothing it was made on
        let entry = String::from_utf8_lossy(entry);
        for session in [entry.to_string(), session_ref(&entry)] {
            c.del(&[
                confirmed_key(&session).as_bytes(),
                confirm_failures_key(&session).as_bytes(),
            ])?;
        }
    }
    c.del(&[idx.as_bytes()])?;
    Ok(entries.len())
}

/// Delete every agent key `address` owns. Returns how many secret indexes
/// pointed at it. The id counter stays, so a later account with the same
/// address never reissues an id an old index could name.
pub fn revoke_agent_keys(c: &mut Connection, address: &str) -> std::io::Result<usize> {
    c.del(&[
        format!("agent:keys:{address}").as_bytes(),
        crate::agent_key_usage::hash_key(address).as_bytes(),
    ])?;
    // the index stores no owner prefix in its key, so finding an owner's
    // entries means reading each one; there are a handful per server
    let mut dropped = 0;
    for idx_key in c.keys(crate::agent_key_index::PATTERN)? {
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
