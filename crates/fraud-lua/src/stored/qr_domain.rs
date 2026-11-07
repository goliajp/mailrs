//! Whether a stored message's QR codes go to a domain new to this
//! deployment: a fact that needs the shared store as well as the message.
//!
//! Read only. Delivery records when each domain was first seen; judging a
//! message, live or stored, never does, so judging it twice cannot change
//! what it is judged against.

use std::sync::LazyLock;

use mailrs_core_sidestate::families::domain_first;

use super::Reading;

/// Shared platforms, which no QR code is new for.
static NEVER: LazyLock<mailrs_reputation::NeverList> =
    LazyLock::new(|| mailrs_reputation::NeverList::builtin(&[]));

/// The registrable domain of the first QR code host in `msg` that was new
/// to this deployment when the message arrived at `at`. Without a store
/// none is new, which errs towards delivering.
pub fn new_domain(
    hist: Option<&mut kevy_client::Connection>,
    msg: &Reading,
    at: i64,
) -> Option<String> {
    let conn = hist?;
    let hosts: Vec<String> = msg
        .qr_hosts
        .iter()
        .filter(|h| !NEVER.contains(h))
        .cloned()
        .collect();
    if hosts.is_empty() {
        return None;
    }
    domain_first::new_hosts(Some(conn), &hosts, at)
        .first()
        .map(|h| mailrs_fraud::brand::registrable(h))
}
