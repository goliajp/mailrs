//! Sessions issued before they were stored by the hash of their token.

use super::{SESSION_TTL_SECS, SessionBlob, session_key};

/// A session issued before sessions were stored by hash, moved to its
/// hashed key on first use.
///
/// The network client has no transaction on every backend, so the steps
/// are ordered so a logout or a revocation running alongside cannot bring
/// the session back: the hashed copy is written first and dropped again if
/// the original has meanwhile been deleted; the index gains the hash before
/// it loses the token, so a revocation always finds one of the two, and
/// revocation deletes the hashed key for either.
pub(super) fn move_legacy_session(
    client: &mut kevy_client::Connection,
    token: &str,
) -> std::io::Result<Option<Vec<u8>>> {
    let legacy = mailrs_core_sidestate::keys::session::legacy_session_key(token);
    let Some(bytes) = client.get(legacy.as_bytes())? else {
        return Ok(None);
    };
    write_moved(client, token, &bytes)?;
    settle_move(client, token, bytes)
}

// the steps of `move_legacy_session`, apart so the tests can end the
// session between them
pub(super) fn write_moved(
    client: &mut kevy_client::Connection,
    token: &str,
    bytes: &[u8],
) -> std::io::Result<()> {
    client.set_with_ttl(
        session_key(token).as_bytes(),
        bytes,
        std::time::Duration::from_secs(SESSION_TTL_SECS),
    )?;
    Ok(())
}

pub(super) fn settle_move(
    client: &mut kevy_client::Connection,
    token: &str,
    bytes: Vec<u8>,
) -> std::io::Result<Option<Vec<u8>>> {
    use mailrs_core_sidestate::keys::session::{by_addr_key, legacy_session_key, session_ref};
    let legacy = legacy_session_key(token);
    let stored = session_key(token);
    if client.exists(&[legacy.as_bytes()])? == 0 {
        client.del(&[stored.as_bytes()])?;
        return Ok(None);
    }
    // the token leaves the index last: while it is there a revocation
    // deletes the old key, which a later request would otherwise move back
    let idx = serde_json::from_slice::<SessionBlob>(&bytes)
        .ok()
        .map(|blob| by_addr_key(&blob.address));
    if let Some(idx) = &idx {
        client.sadd(idx.as_bytes(), &[session_ref(token).as_bytes()])?;
    }
    client.del(&[legacy.as_bytes()])?;
    if let Some(idx) = &idx {
        client.srem(idx.as_bytes(), &[token.as_bytes()])?;
    }
    Ok(Some(bytes))
}
