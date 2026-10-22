//! The one Sieve script a user has: the network kevy `sieve:<address>`
//! key, which the spool drain reads at delivery and webapi's admin API
//! writes.

use kevy_client::Connection;

use crate::FastcoreState;

fn kevy(state: &FastcoreState) -> std::io::Result<Connection> {
    state
        .net_conn()
        .ok_or_else(|| std::io::Error::other("the script store is unreachable"))
}

pub(super) fn store_script(state: &FastcoreState, user: &str, body: &str) -> std::io::Result<()> {
    kevy(state)?.set(
        mailrs_core_sidestate::sieve_key(user).as_bytes(),
        body.as_bytes(),
    )?;
    Ok(())
}

/// `Ok(None)` when the user has no script; an unreachable store is an
/// error, so it is never reported as a missing script.
pub(super) fn load_script(state: &FastcoreState, user: &str) -> std::io::Result<Option<String>> {
    let raw = kevy(state)?.get(mailrs_core_sidestate::sieve_key(user).as_bytes())?;
    Ok(raw
        .and_then(|v| String::from_utf8(v).ok())
        .filter(|s| !s.trim().is_empty()))
}

pub(super) fn delete_script(state: &FastcoreState, user: &str) -> std::io::Result<()> {
    kevy(state)?.del(&[mailrs_core_sidestate::sieve_key(user).as_bytes()])?;
    Ok(())
}
