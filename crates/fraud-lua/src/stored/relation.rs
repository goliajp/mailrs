//! The reader's history with a sender, read from the shared contact store
//! for [`mailrs_fraud::relation::assess`].
//!
//! The same reading for the live path and the sweep, apart from one
//! thing: on the live path this message is not counted yet, in a sweep it
//! already is.

use std::sync::LazyLock;

use mailrs_core_sidestate::families::{contact_cards, contact_sent, contacts, payment_accounts};
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

/// `user`'s history with the sender of `from` (the decoded header), and
/// whether `user` has written to `reply_to` (the bare `Reply-To` address,
/// empty without one). Nothing known — no store — is all false: no
/// evidence either way.
pub fn relation(
    conn: Option<&mut kevy_client::Connection>,
    user: &str,
    from: &str,
    reply_to: &str,
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
    Relation {
        reply_to_correspondent: mailrs_fraud::relation::wrote_to(
            &reply_to.trim().to_ascii_lowercase(),
            &correspondents,
        ),
        ..mailrs_fraud::relation::assess(
            &address,
            &fold(&display_name_of(from)),
            &correspondents,
            Some(history),
            |d| NEVER.contains(d),
        )
    }
}

/// Whether the message from `from` that arrived at `at`, carrying the
/// payment accounts `keys`, gives `user` an account its sender had not
/// given before, from a sender that had given another
/// ([`payment_accounts::changed`]). Read by arrival, so the live path and
/// a sweep over stored mail, already recorded, read it alike. Without a
/// store, or when it cannot be read, no.
pub fn account_changed(
    conn: Option<&mut kevy_client::Connection>,
    user: &str,
    from: &str,
    keys: &[String],
    at: i64,
) -> bool {
    let Some(conn) = conn else {
        return false;
    };
    if user.is_empty() {
        return false;
    }
    payment_accounts::changed(conn, user, from, keys, at).unwrap_or(false)
}

/// Every address `user` has sent mail to (`sent_count` above zero), with
/// the name the autocomplete store shows for it.
///
/// Not the autocomplete store's addresses: every delivery adds its sender
/// there, so it holds whoever wrote in, forgers included.
fn correspondents(conn: &mut kevy_client::Connection, user: &str) -> Vec<Correspondent> {
    let sent_to = contact_sent::sent_to(conn, user).unwrap_or_default();
    if sent_to.is_empty() {
        return Vec::new();
    }
    let flat = conn
        .hgetall(contact_cards::autocomplete_key(user).as_bytes())
        .unwrap_or_default();
    let shown: std::collections::HashMap<String, String> = flat
        .chunks_exact(2)
        .map(|pair| {
            (
                String::from_utf8_lossy(&pair[0]).to_ascii_lowercase(),
                String::from_utf8_lossy(&pair[1]).into_owned(),
            )
        })
        .collect();
    sent_to
        .into_iter()
        .map(|address| {
            let address = address.to_ascii_lowercase();
            let name = shown
                .get(&address)
                .map(|s| fold(&display_name_of(s)))
                .unwrap_or_default();
            Correspondent { address, name }
        })
        .collect()
}
