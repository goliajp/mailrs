//! Domains a reader has let through from the burner-domain rule by
//! marking a message not junk.
//!
//! ```text
//! mailrs:burnerreleased:{user}   set of registrable domains
//! ```
//!
//! When a reader marks not junk a message from a sender whose web root
//! lands on another domain ([`super::site_landing`]), both the sender's
//! registrable domain and the one it lands on are added: the reader has
//! said that company is real, so its other addresses and its other
//! sending domains that point at the same site are not cold outreach to
//! them. Marking such a message junk takes both away again; the last mark
//! wins. One reader's set; nobody else's mail is affected.
//!
//! Written by the web tier when a reader marks a conversation, read by
//! every judging of a message for that reader
//! ([`mailrs_fraud::burner_domain`] facts in `mailrs-fraud-lua`).

use std::io;

use kevy_client::Connection;
use mailrs_fraud::brand::registrable;

use super::site_landing;

/// The key of `user`'s set.
pub fn key(user: &str) -> String {
    format!("mailrs:burnerreleased:{}", user.trim().to_ascii_lowercase())
}

/// The domains a mark on mail from `from_host` speaks for: its
/// registrable domain and, when its web root lands on another registrable
/// domain, that one. Empty for a sender whose site does not land
/// elsewhere, or has not been visited: the rule never scored it.
fn marked_domains(conn: &mut Connection, from_host: &str) -> io::Result<Vec<String>> {
    let from = registrable(from_host);
    let landed = site_landing::get(conn, from_host)?
        .map(|l| l.registrable())
        .unwrap_or_default();
    if from.is_empty() || landed.is_empty() || landed == from {
        return Ok(Vec::new());
    }
    Ok(vec![from, landed])
}

/// `user` marked mail from `from_host` not junk. Returns the domains added.
pub fn released(conn: &mut Connection, user: &str, from_host: &str) -> io::Result<Vec<String>> {
    let domains = marked_domains(conn, from_host)?;
    if !domains.is_empty() {
        let members: Vec<&[u8]> = domains.iter().map(|d| d.as_bytes()).collect();
        conn.sadd(key(user).as_bytes(), &members)?;
    }
    Ok(domains)
}

/// `user` marked mail from `from_host` junk: whatever an earlier release
/// added for it goes. Returns the domains removed from consideration.
pub fn junked(conn: &mut Connection, user: &str, from_host: &str) -> io::Result<Vec<String>> {
    let domains = marked_domains(conn, from_host)?;
    if !domains.is_empty() {
        let members: Vec<&[u8]> = domains.iter().map(|d| d.as_bytes()).collect();
        conn.srem(key(user).as_bytes(), &members)?;
    }
    Ok(domains)
}

/// Whether `user` has released the registrable `domain`.
pub fn knows(conn: &mut Connection, user: &str, domain: &str) -> io::Result<bool> {
    if domain.is_empty() {
        return Ok(false);
    }
    conn.sismember(key(user).as_bytes(), domain.as_bytes())
        .map_err(io::Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailrs_fraud::burner_domain::Landing;

    const ME: &str = "lihao@golia.jp";

    fn conn(name: &str) -> Connection {
        let mut c = Connection::connect(&format!("mem://burner-released-{name}")).unwrap();
        for (host, at) in [
            ("dealclosersvoice.com", "dealclosers.co"),
            ("247dealclosers.com", "www.dealclosers.co"),
            ("example.com", "www.example.com"),
        ] {
            site_landing::put(&mut c, host, &Landing::At(at.into())).unwrap();
        }
        c
    }

    #[test]
    fn a_release_adds_the_sender_and_where_its_site_lands() {
        let mut c = conn("release");
        let added = released(&mut c, ME, "dealclosersvoice.com").unwrap();
        assert_eq!(added, ["dealclosersvoice.com", "dealclosers.co"]);
        assert!(knows(&mut c, ME, "dealclosersvoice.com").unwrap());
        assert!(knows(&mut c, ME, "dealclosers.co").unwrap());
        // only for the reader who marked it
        assert!(!knows(&mut c, "tj@golia.jp", "dealclosers.co").unwrap());
    }

    #[test]
    fn a_sender_whose_site_stays_home_or_was_never_visited_adds_nothing() {
        let mut c = conn("home");
        assert!(released(&mut c, ME, "example.com").unwrap().is_empty());
        assert!(
            released(&mut c, ME, "never-visited.com")
                .unwrap()
                .is_empty()
        );
        assert!(!knows(&mut c, ME, "example.com").unwrap());
    }

    #[test]
    fn the_last_mark_wins() {
        let mut c = conn("last");
        released(&mut c, ME, "dealclosersvoice.com").unwrap();
        // junk from a sister domain landing on the same site takes the
        // landing back, and leaves the domain that was released itself
        junked(&mut c, ME, "247dealclosers.com").unwrap();
        assert!(!knows(&mut c, ME, "dealclosers.co").unwrap());
        assert!(knows(&mut c, ME, "dealclosersvoice.com").unwrap());
        junked(&mut c, ME, "dealclosersvoice.com").unwrap();
        assert!(!knows(&mut c, ME, "dealclosersvoice.com").unwrap());
        released(&mut c, ME, "dealclosersvoice.com").unwrap();
        assert!(knows(&mut c, ME, "dealclosers.co").unwrap());
    }
}
