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

use crate::handlers::kevy_util::with_kevy;

pub use mailrs_core_sidestate::keys::session::{index_session, revoke_sessions};

/// Delete every agent key `address` owns. Returns how many secret indexes
/// pointed at it. The id counter stays, so a later account with the same
/// address never reissues an id an old index could name.
pub fn revoke_agent_keys(c: &mut Connection, address: &str) -> std::io::Result<usize> {
    c.del(&[
        mailrs_core_sidestate::keys::credentials::agent_keys_key(address).as_bytes(),
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
