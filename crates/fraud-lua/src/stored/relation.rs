//! The reader's history with a sender, read from the shared contact store
//! for [`mailrs_fraud::relation::assess`].
//!
//! The same reading for the live path and the sweep, apart from one
//! thing: on the live path this message is not counted yet, in a sweep it
//! already is.

use std::sync::LazyLock;

use mailrs_core_sidestate::families::{contact_cards, contacts};
use mailrs_fraud::impersonation::{display_name_of, fold};
use mailrs_fraud::relation::{Correspondent, History, Relation};

/// Shared platforms, which are nobody's correspondent domain.
static NEVER: LazyLock<mailrs_reputation::NeverList> =
    LazyLock::new(|| mailrs_reputation::NeverList::builtin(&[]));

/// When the message is judged: before its own arrival is counted (the
/// live path) or after (a sweep over stored mail).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Counted {
    /// The live path: this message is not in the counts yet.
    Before,
    /// A sweep: this message is already counted.
    After,
}

/// `user`'s history with the sender of `from` (the decoded header).
/// Nothing known — no store — is all false: no evidence either way.
pub fn relation(
    conn: Option<&mut kevy_client::Connection>,
    user: &str,
    from: &str,
    counted: Counted,
) -> Relation {
    let Some(conn) = conn else {
        return Relation::default();
    };
    let address = mailrs_fraud::impersonation::address_of(from)
        .unwrap_or("")
        .trim()
        .trim_end_matches('>')
        .to_ascii_lowercase();
    if user.is_empty() || address.is_empty() {
        return Relation::default();
    }
    let scoring = contacts::scoring_for(conn, user, &address);
    let already = match counted {
        Counted::Before => 0,
        Counted::After => 1,
    };
    let history = History {
        received: scoring.received_count.saturating_sub(already),
        sent: scoring.sent_count,
        accepted: contacts::accepted(conn, user, &address),
    };
    let correspondents = correspondents(conn, user);
    mailrs_fraud::relation::assess(
        &address,
        &fold(&display_name_of(from)),
        &correspondents,
        Some(history),
        |d| NEVER.contains(d),
    )
}

/// Everyone `user` has written to, from the autocomplete store (address
/// and the name it was written as).
fn correspondents(conn: &mut kevy_client::Connection, user: &str) -> Vec<Correspondent> {
    let flat = conn
        .hgetall(contact_cards::autocomplete_key(user).as_bytes())
        .unwrap_or_default();
    flat.chunks_exact(2)
        .map(|pair| {
            let shown = String::from_utf8_lossy(&pair[1]);
            Correspondent {
                address: String::from_utf8_lossy(&pair[0]).to_ascii_lowercase(),
                name: fold(&display_name_of(&shown)),
            }
        })
        .collect()
}
