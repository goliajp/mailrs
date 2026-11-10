//! The reader's history with a sender, read from the shared contact store
//! for [`mailrs_fraud::relation::assess`].
//!
//! The same reading for the live path and the sweep, apart from one
//! thing: on the live path this message is not counted yet, in a sweep it
//! already is.

use std::sync::LazyLock;

use mailrs_core_sidestate::families::{contact_cards, contact_sent, contacts, payment_accounts};
use mailrs_fraud::brand::registrable;
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

/// Whether `user` has sent mail to someone at the registrable `domain`:
/// the test `from_correspondent_domain` makes of the sender's domain, for
/// another one (where the sender's web root lands). A shared platform is
/// nobody's domain. Without a store, or for an empty domain, no.
pub fn wrote_to_domain(conn: &mut kevy_client::Connection, user: &str, domain: &str) -> bool {
    if user.is_empty() || domain.is_empty() || NEVER.contains(domain) {
        return false;
    }
    correspondents(conn, user)
        .iter()
        .filter_map(|c| c.address.rsplit_once('@').map(|(_, h)| registrable(h)))
        .any(|d| d == domain)
}

/// Whether the message's `To` or `Cc`, `user` aside, names someone `user`
/// has sent mail to, or a colleague: an address at one of `our_domains`
/// that mail has passed between and `user`, either way.
///
/// A first message from a real company is often an introduction, copying
/// people on both sides; cold outreach is one to one. Being at our domain
/// is not enough to be a colleague: every address there that is not an
/// account delivers to an account, so a letter to `hao@` is a letter to
/// the reader under another name, and it never writes back.
pub fn copies_someone_known(
    conn: &mut kevy_client::Connection,
    user: &str,
    raw: &[u8],
    our_domains: &[String],
) -> bool {
    let user = user.to_ascii_lowercase();
    let copied: Vec<String> = [b"to:".as_slice(), b"cc:".as_slice()]
        .iter()
        .filter_map(|h| super::header_value(raw, h))
        .flat_map(|v| addresses_in(&v))
        .filter(|a| *a != user)
        .collect();
    if copied.is_empty() {
        return false;
    }
    let ours: Vec<String> = our_domains.iter().map(|d| registrable(d)).collect();
    let colleague = copied.iter().any(|a| {
        a.rsplit_once('@')
            .is_some_and(|(_, h)| ours.contains(&registrable(h)))
            && {
                let s = contacts::scoring_for(conn, &user, a);
                s.received_count > 0 || s.sent_count > 0
            }
    });
    if colleague {
        return true;
    }
    let correspondents = correspondents(conn, &user);
    copied
        .iter()
        .any(|a| mailrs_fraud::relation::wrote_to(a, &correspondents))
}

/// The addresses in an address-list header value, lowercased: every
/// `<…>` that holds one, and every bare word with an `@`.
fn addresses_in(value: &str) -> Vec<String> {
    value
        .split(|c: char| matches!(c, ',' | ';' | '<' | '>' | '"' | '(' | ')') || c.is_whitespace())
        .map(|t| {
            t.trim_matches(|c: char| c == '.' || c == '\'')
                .to_ascii_lowercase()
        })
        .filter(|t| {
            t.split_once('@')
                .is_some_and(|(l, h)| !l.is_empty() && h.contains('.'))
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use mailrs_core_sidestate::families::contact_sent::set_counts;

    const ME: &str = "lihao@golia.jp";

    fn conn(name: &str) -> kevy_client::Connection {
        let mut c = kevy_client::Connection::connect(&format!("mem://relation-{name}")).unwrap();
        set_counts(&mut c, ME, "suzuki@example.com", 3, 2).unwrap();
        // heard from, never written to: not someone known
        set_counts(&mut c, ME, "stranger@other.com", 5, 0).unwrap();
        // a colleague who has written to the reader
        set_counts(&mut c, ME, "tanaka@golia.ai", 4, 0).unwrap();
        c
    }

    fn ours() -> Vec<String> {
        vec!["golia.jp".into(), "golia.ai".into()]
    }

    #[test]
    fn a_domain_the_reader_wrote_to_is_known_and_others_are_not() {
        let mut c = conn("domain");
        assert!(wrote_to_domain(&mut c, ME, "example.com"));
        assert!(!wrote_to_domain(&mut c, ME, "other.com"));
        assert!(!wrote_to_domain(&mut c, ME, ""));
    }

    #[test]
    fn a_copied_correspondent_or_colleague_is_someone_known() {
        let mut c = conn("copies");
        let intro = b"From: yamada@example-group.co.jp\r\nTo: \"Li, Hao\" <lihao@golia.jp>\r\nCc: Suzuki <SUZUKI@example.com>\r\n\r\nbody";
        assert!(copies_someone_known(&mut c, ME, intro, &ours()));
        let colleague = b"From: a@x.com\r\nTo: lihao@golia.jp, tanaka@golia.ai\r\n\r\nbody";
        assert!(copies_someone_known(&mut c, ME, colleague, &ours()));
    }

    #[test]
    fn the_reader_alone_or_strangers_on_copy_are_not() {
        let mut c = conn("alone");
        let one_to_one = b"From: a@x.com\r\nTo: Li Hao <lihao@golia.jp>\r\n\r\nCc: suzuki@example.com in the body is not a header";
        assert!(!copies_someone_known(&mut c, ME, one_to_one, &ours()));
        let strangers = b"From: a@x.com\r\nTo: lihao@golia.jp\r\nCc: stranger@other.com,\r\n\tnobody@nowhere.net\r\n\r\nbody";
        assert!(!copies_someone_known(&mut c, ME, strangers, &ours()));
        // an address at our domain nothing has passed between is the
        // reader's alias as often as a colleague: the catch-all delivers it
        let alias = b"From: a@x.com\r\nTo: hao@golia.jp\r\n\r\nbody";
        assert!(!copies_someone_known(&mut c, ME, alias, &ours()));
    }
}
