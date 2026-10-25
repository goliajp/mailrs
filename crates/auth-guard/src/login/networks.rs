//! One account signing in from several networks in a few minutes: often
//! a password someone else now has.

use std::io;

use super::{Ops, Scopes, loggable_account};
use crate::alerts::{self, Alert, Kind};
use crate::events::EventOps;

/// Networks seen signing in to one account within this long...
const RECENT_SECS: u64 = 600;
/// ...that make an alert.
const RECENT_NETWORKS: usize = 3;

/// Raise [`Kind::ManyNetworks`] when this sign-in makes
/// [`RECENT_NETWORKS`] networks within [`RECENT_SECS`].
pub(super) fn many_networks<O: Ops + EventOps>(
    ops: &mut O,
    s: &Scopes,
    account: &str,
    now: u64,
) -> io::Result<()> {
    let key = super::recent_key(&s.account);
    ops.hset(&key, &s.familiar_field, now, RECENT_SECS)?;
    let recent: Vec<String> = ops
        .hgetall(&key)?
        .into_iter()
        .filter(|(_, at)| at.saturating_add(RECENT_SECS) > now)
        .map(|(net, _)| net)
        .collect();
    if recent.len() >= RECENT_NETWORKS {
        let account = loggable_account(account);
        let detail = format!(
            "signed in successfully from {} networks within {} minutes: {}",
            recent.len(),
            RECENT_SECS / 60,
            recent.join(", ")
        );
        let alert = Alert {
            kind: Kind::ManyNetworks,
            subject: account,
            at: now,
            detail,
        };
        alerts::raise(ops, &alert)?;
    }
    Ok(())
}
