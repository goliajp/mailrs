//! Where a Sieve script lives: `sieve:<address>` in the network kevy.
//!
//! Not on `DomainStore`, which is the SQL store, and this is not SQL data. It
//! sits in the shared network store beside sessions, the greylist and contacts
//! — shared meaning both cores read the same key, so a script does the same
//! thing whichever core is serving.
//!
//! It used to be PG `sieve_scripts`, and the split was silent in both
//! directions. Every live reader looks at the kevy key: the core-api contract's
//! own GET (`core-sidestate/families/groups_admin.rs`), the web UI's save path
//! (`webapi/handlers/admin_ops.rs` writes it directly), fastcore's ManageSieve,
//! and fastcore's delivery-time evaluator (`fastcore/src/sieve_apply.rs`). Only
//! this crate's three call sites used the table. So with the SQL core serving
//! ManageSieve and delivery, a script saved in the UI would have filtered no
//! mail, and one saved over ManageSieve would have been invisible to the UI —
//! neither raising an error, which is `rules/one-side-of-the-wire.md` exactly.
//!
//! PG `sieve_scripts` now has no reader and no writer. The table stays in
//! `init-schema.sql` — dropping it is a migration and it costs nothing empty —
//! but nothing should start using it again.
//!
//! `MAILRS_KEVY_URL` per call, and a connection per call, because that is what
//! `fastcore/src/sieve_apply.rs::fetch_script` already does on the lane that
//! serves production today. Matching it is the point: the two lanes should
//! reach this key the same way. If per-message connects turn out to cost, they
//! cost on both lanes and get fixed on both.

use kevy_client::Connection;

fn conn_at(url: &str) -> Option<Connection> {
    Connection::connect(url).ok()
}

/// The one place this crate spells the key.
///
/// Byte-identical to `mailrs_core_sidestate::sieve_key`, and NOT an import of
/// it: that crate is an optional dependency here, gated on `core-rpc`, so the
/// default build cannot see it — and making it unconditional would change the
/// dependency graph of an artifact that is meant to be unaffected by the
/// feature. Two spellings of one fact is the defect this module exists to fix,
/// so `scripts/check-outbound-keys.sh` holds these two to being identical.
fn key(address: &str) -> String {
    format!("sieve:{address}")
}

/// The account's script. `Ok(None)` when unset or empty; `Err` when the store
/// could not be reached.
///
/// `Result` rather than a bare `Option`, so "there is no filter" and "I could
/// not find out" stay distinguishable. The delivery path is entitled to treat
/// them alike — it asks per message and the alternative to not filtering is
/// refusing the mail — and it already does, at its own call site. ManageSieve
/// and the admin API are not entitled to: answering GETSCRIPT with "no script"
/// because the store was down is how someone concludes their filter was
/// deleted.
pub fn get(address: &str) -> Result<Option<String>, std::io::Error> {
    get_at(
        &std::env::var("MAILRS_KEVY_URL").map_err(|_| unreachable())?,
        address,
    )
}

fn get_at(url: &str, address: &str) -> Result<Option<String>, std::io::Error> {
    let raw = conn_at(url)
        .ok_or_else(unreachable)?
        .get(key(address).as_bytes())?;
    Ok(raw
        .and_then(|v| String::from_utf8(v).ok())
        .filter(|s| !s.trim().is_empty()))
}

fn unreachable() -> std::io::Error {
    std::io::Error::other("no MAILRS_KEVY_URL, or the network store is unreachable")
}

/// Save the script. `Err` when the store is unreachable — a save that silently
/// did nothing is worse than a visible failure, since the person then believes
/// their filter is live.
pub fn set(address: &str, script: &str) -> Result<(), std::io::Error> {
    set_at(
        &std::env::var("MAILRS_KEVY_URL").map_err(|_| unreachable())?,
        address,
        script,
    )
}

/// Delete the script. `Ok(true)` when one was there.
pub fn delete(address: &str) -> Result<bool, std::io::Error> {
    delete_at(
        &std::env::var("MAILRS_KEVY_URL").map_err(|_| unreachable())?,
        address,
    )
}

fn set_at(url: &str, address: &str, script: &str) -> Result<(), std::io::Error> {
    conn_at(url)
        .ok_or_else(unreachable)?
        .set(key(address).as_bytes(), script.as_bytes())?;
    Ok(())
}

fn delete_at(url: &str, address: &str) -> Result<bool, std::io::Error> {
    let n = conn_at(url)
        .ok_or_else(unreachable)?
        .del(&[key(address).as_bytes()])?;
    Ok(n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_the_one_every_other_reader_uses() {
        // Spelled out rather than derived, because the whole defect this module
        // fixes was two spellings of one fact. If this changes, it has to change
        // in core-sidestate, webapi and fastcore in the same commit.
        assert_eq!(key("a@b.test"), "sieve:a@b.test");
    }

    // A port nothing listens on. These go through `conn_at` rather than
    // setting MAILRS_KEVY_URL, because mutating the environment is racy
    // between tests sharing a binary — and a racy test that occasionally
    // measures a neighbour's state is worse than no test.
    const NOWHERE: &str = "kevy://127.0.0.1:1";

    #[test]
    fn unreachable_store_is_an_error_not_an_empty_answer() {
        // The distinction this module keeps: an unreachable store must not read
        // as "this account has no filter". Callers that want to treat the two
        // alike say so themselves.
        assert!(get_at(NOWHERE, "a@b.test").is_err());
    }

    #[test]
    fn unreachable_store_fails_a_write_loudly() {
        // The asymmetry is the point: a read may treat "no store" as "no
        // script", but a write must not, or the person believes their filter is
        // live when nothing was stored.
        assert!(
            set_at(NOWHERE, "a@b.test", "keep;").is_err(),
            "a save with nowhere to go must report it, not return Ok"
        );
        assert!(delete_at(NOWHERE, "a@b.test").is_err());
    }
}
