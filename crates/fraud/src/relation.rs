//! The reader's history with a sender, as facts the rules may weigh.
//!
//! Every fact here is an opinion of the sender, built from who the reader
//! has written to and heard from — so a rule may only call a message
//! suspicious on them, never hold it: familiarity is not proof.

use crate::brand::registrable;

/// One address the reader has sent mail to, and the name it goes by.
///
/// Only an address the reader wrote to is a correspondent. Mail that
/// arrived from an address makes it known, never a correspondent: anyone,
/// a forger included, can write in.
#[derive(Debug, Clone)]
pub struct Correspondent {
    /// Lowercased address.
    pub address: String,
    /// The display name, folded the way display names are compared.
    pub name: String,
}

/// What the store knows of this sender.
#[derive(Debug, Clone, Copy, Default)]
pub struct History {
    /// Messages received from this address before this one.
    pub received: u32,
    /// Messages the reader sent to it.
    pub sent: u32,
    /// The reader accepted the sender as no longer new.
    pub accepted: bool,
}

/// The relationship facts for one message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Relation {
    /// Nothing has passed between the reader and this address.
    pub first_contact: bool,
    /// The reader has sent mail to this very address.
    pub from_correspondent: bool,
    /// The reader has sent mail to this message's `Reply-To` address.
    /// False without one. Set by the host, which knows the header;
    /// [`assess`] reads only the `From:`.
    pub reply_to_correspondent: bool,
    /// The reader has sent mail to some address at the sender's
    /// registrable domain. A shared platform (the never-list: gmail.com
    /// and the like) is nobody's domain, so it is never true there.
    pub from_correspondent_domain: bool,
    /// The sender's registrable domain is two edits or a look-alike
    /// letter away from one the reader has sent mail to, without being it.
    pub looks_like_correspondent: bool,
    /// The correspondent's domain it looks like, when it does.
    pub lookalike_of: Option<String>,
    /// The display name is a correspondent's, at an address that has
    /// never been in touch.
    pub name_borrowed: bool,
}

/// Weigh `from_address` (lowercased) and its folded `display` name
/// against what the reader has written to and the sender's own history.
#[must_use]
pub fn assess(
    from_address: &str,
    display: &str,
    correspondents: &[Correspondent],
    history: Option<History>,
    never_listed: impl Fn(&str) -> bool,
) -> Relation {
    let first_contact = match history {
        Some(h) => h.received == 0 && h.sent == 0 && !h.accepted,
        // nothing known is not evidence of a first contact
        None => false,
    };
    let sender_host = from_address.rsplit_once('@').map_or("", |(_, h)| h);
    let sender = registrable(sender_host);
    let domains: Vec<String> = correspondents
        .iter()
        .filter_map(|c| c.address.rsplit_once('@').map(|(_, h)| registrable(h)))
        .filter(|d| !d.is_empty() && !never_listed(d))
        .collect();
    let lookalike_of = domains
        .iter()
        .find(|d| **d != sender && (close(d, &sender) || unconfuse(d) == unconfuse(&sender)))
        .filter(|_| !sender.is_empty())
        .cloned();
    let name_borrowed = first_contact
        && !display.trim().is_empty()
        && correspondents
            .iter()
            .any(|c| c.address != from_address && !c.name.is_empty() && c.name == display);
    Relation {
        first_contact,
        from_correspondent: wrote_to(from_address, correspondents),
        from_correspondent_domain: !sender.is_empty() && domains.contains(&sender),
        looks_like_correspondent: lookalike_of.is_some(),
        lookalike_of,
        name_borrowed,
        reply_to_correspondent: false,
    }
}

/// Whether `address` (lowercased) is one the reader has sent mail to.
#[must_use]
pub fn wrote_to(address: &str, correspondents: &[Correspondent]) -> bool {
    !address.is_empty() && correspondents.iter().any(|c| c.address == address)
}

/// Within two edits (insert, delete, substitute, swap), on domains long
/// enough that two edits do not turn one short name into another.
fn close(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().min(b.len()) < 8 || a.len().abs_diff(b.len()) > 2 {
        return false;
    }
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = best;
        }
    }
    d[a.len()][b.len()] <= 2
}

/// The letters a look-alike swaps in, folded back.
fn unconfuse(domain: &str) -> String {
    domain
        .replace("rn", "m")
        .replace("vv", "w")
        .replace('0', "o")
        .replace('1', "l")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(address: &str, name: &str) -> Correspondent {
        Correspondent {
            address: address.into(),
            name: name.into(),
        }
    }

    fn never(d: &str) -> bool {
        d == "gmail.com"
    }

    const NEW: Option<History> = Some(History {
        received: 0,
        sent: 0,
        accepted: false,
    });

    #[test]
    fn first_contact_needs_the_history_to_say_so() {
        let known = Some(History {
            received: 3,
            ..History::default()
        });
        assert!(assess("a@x.example", "", &[], NEW, never).first_contact);
        assert!(!assess("a@x.example", "", &[], known, never).first_contact);
        assert!(!assess("a@x.example", "", &[], None, never).first_contact);
        let accepted = Some(History {
            accepted: true,
            ..History::default()
        });
        assert!(!assess("a@x.example", "", &[], accepted, never).first_contact);
    }

    #[test]
    fn a_look_alike_of_a_correspondent_domain_is_found() {
        let people = [c("tanaka@acme-trading.co.jp", "田中 太郎")];
        let r = assess("billing@acme-tradlng.co.jp", "", &people, NEW, never);
        assert!(r.looks_like_correspondent);
        assert!(!r.from_correspondent_domain);
        let r = assess(
            "x@rnicrosoft-partner.example",
            "",
            &[c("a@microsoft-partner.example", "")],
            NEW,
            never,
        );
        assert!(r.looks_like_correspondent);
        // the domain itself is not a look-alike of itself
        let r = assess("sato@acme-trading.co.jp", "", &people, NEW, never);
        assert!(r.from_correspondent_domain && !r.looks_like_correspondent);
        // never-listed domains are not anyone's
        let r = assess("x@gmall.com", "", &[c("a@gmail.com", "")], NEW, never);
        assert!(!r.looks_like_correspondent);
        // short names are too close to each other to tell
        let r = assess("x@abd.jp", "", &[c("a@abc.jp", "")], NEW, never);
        assert!(!r.looks_like_correspondent);
    }

    /// The address itself counts even on a shared platform, where the
    /// domain is nobody's.
    #[test]
    fn the_exact_address_written_to_is_a_correspondent() {
        let people = [c("friend@gmail.com", ""), c("tanaka@acme.example", "")];
        let r = assess("friend@gmail.com", "", &people, NEW, never);
        assert!(r.from_correspondent && !r.from_correspondent_domain);
        let r = assess("sato@acme.example", "", &people, NEW, never);
        assert!(!r.from_correspondent && r.from_correspondent_domain);
        let r = assess("other@gmail.com", "", &people, NEW, never);
        assert!(!r.from_correspondent && !r.from_correspondent_domain);
    }

    #[test]
    fn a_borrowed_name_is_a_correspondent_name_at_a_new_address() {
        let people = [c("tanaka@acme.example", "田中 太郎")];
        assert!(assess("tanaka.acme@free.example", "田中 太郎", &people, NEW, never).name_borrowed);
        let known = Some(History {
            sent: 1,
            ..History::default()
        });
        assert!(
            !assess(
                "tanaka.acme@free.example",
                "田中 太郎",
                &people,
                known,
                never
            )
            .name_borrowed
        );
        assert!(!assess("other@free.example", "佐藤", &people, NEW, never).name_borrowed);
    }
}
