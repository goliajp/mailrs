//! Sending as a mailbox somewhere else.
//!
//! Delivering to an MX needs no credential and is what this sender
//! does for its own domains. A message whose envelope sender is a
//! **connected** mailbox cannot go that way: sent from our IP as
//! `someone@gmail.com` it fails SPF and DMARC at every receiver, and
//! is refused or filed as spam.
//!
//! So it is submitted through the provider's own server with that
//! account's credential, which is what a mail client does — and what
//! the account was connected as.

use std::sync::Arc;

use mailrs_core_sidestate::families::external_accounts::{self as ext, AccountRow};

/// Where and how to submit one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Submission {
    /// Host to connect to.
    pub host: String,
    /// Port.
    pub port: u16,
    /// Whether TLS is there from the first byte.
    pub implicit_tls: bool,
    /// Login name — the account's own, unless it set another.
    pub user: String,
    /// Which account's secret opens this connection.
    pub account_id: String,
}

/// The account to submit as, if the envelope sender is a connected one.
///
/// Matched case-insensitively on the address, because a person types
/// their own address in whatever case they please and the queue carries
/// what they typed.
pub(super) fn submission_for(sender: &str, rows: &[AccountRow]) -> Option<Submission> {
    let row = rows.iter().find(|r| r.email.eq_ignore_ascii_case(sender))?;
    // An account whose credential was refused would fail here too, and
    // failing at the provider is worse than failing at home: some of
    // them count the attempts and lock the account.
    if row.state == ext::State::NeedsAuth {
        return None;
    }
    if row.outgoing.host.trim().is_empty() || row.outgoing.port == 0 {
        return None;
    }
    Some(Submission {
        account_id: row.id.clone(),
        host: row.outgoing.host.clone(),
        port: row.outgoing.port,
        implicit_tls: row.outgoing.tls == ext::Tls::Implicit,
        user: row.username.clone().unwrap_or_else(|| row.email.clone()),
    })
}

/// Every connected account of every user, for the sender to match a
/// queued envelope against.
///
/// The queue row carries an address and not a user, so the lookup is by
/// address across all of them — which is also why two accounts holding
/// the same address would be ambiguous. They are not: an address is
/// connected by whoever holds its password, and if two people connect
/// the same mailbox either credential sends the same mail from the same
/// place.
pub(super) fn all_accounts(kevy_url: &str) -> Vec<AccountRow> {
    let Ok(mut conn) = kevy_client::Connection::connect(kevy_url) else {
        return Vec::new();
    };
    let Ok(keys) = conn.keys(b"ext:accts:*") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for key in keys {
        let Ok(flat) = conn.hgetall(&key) else {
            continue;
        };
        let mut i = 0;
        while i + 1 < flat.len() {
            if let Ok(row) = serde_json::from_slice::<AccountRow>(&flat[i + 1]) {
                out.push(row);
            }
            i += 2;
        }
    }
    out
}

/// The account's password, opened.
pub(super) fn open_secret(
    kevy_url: &str,
    key: &Arc<mailrs_secretbox::Key>,
    id: &str,
) -> Option<String> {
    let mut conn = kevy_client::Connection::connect(kevy_url).ok()?;
    // The secret is keyed by user as well as id, and the sender matched
    // on address alone — so the user is whichever key holds this id.
    let names = conn.keys(b"ext:secret:*").ok()?;
    let suffix = format!(":{id}");
    let name = names
        .into_iter()
        .find(|k| String::from_utf8_lossy(k).ends_with(&suffix))?;
    let sealed = conn.get(&name).ok()??;
    let sealed = String::from_utf8(sealed).ok()?;
    let opened = mailrs_secretbox::open(key, &sealed).ok()?;
    String::from_utf8(opened).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(email: &str, host: &str, port: u16) -> AccountRow {
        AccountRow {
            id: "acc".into(),
            email: email.into(),
            outgoing: ext::Endpoint {
                protocol: "smtp".into(),
                host: host.into(),
                port,
                tls: ext::Tls::Implicit,
            },
            ..AccountRow::default()
        }
    }

    #[test]
    fn mail_from_our_own_domain_is_not_submitted_anywhere() {
        let rows = [row("me@gmail.com", "smtp.gmail.com", 587)];
        assert_eq!(submission_for("me@golia.jp", &rows), None);
    }

    #[test]
    fn a_connected_address_is_submitted_through_its_provider() {
        let rows = [row("me@gmail.com", "smtp.gmail.com", 587)];
        let s = submission_for("me@gmail.com", &rows).expect("a submission");
        assert_eq!(s.host, "smtp.gmail.com");
        assert_eq!(s.port, 587);
        assert_eq!(s.user, "me@gmail.com");
    }

    /// People type their own address in whatever case they please, and
    /// the queue carries what they typed.
    #[test]
    fn the_address_is_matched_without_regard_to_case() {
        let rows = [row("Me@GMail.com", "smtp.gmail.com", 587)];
        assert!(submission_for("me@gmail.com", &rows).is_some());
    }

    /// Failing at the provider is worse than failing at home: some of
    /// them count the attempts and lock the account.
    #[test]
    fn an_account_with_a_refused_password_is_not_tried_again_here() {
        let mut r = row("me@qq.com", "smtp.qq.com", 465);
        r.state = ext::State::NeedsAuth;
        assert_eq!(submission_for("me@qq.com", &[r]), None);
    }

    /// A half-filled row would connect to nowhere on port zero.
    #[test]
    fn an_account_with_no_outgoing_server_is_skipped() {
        assert_eq!(
            submission_for("me@x.com", &[row("me@x.com", "", 587)]),
            None
        );
        assert_eq!(submission_for("me@x.com", &[row("me@x.com", "h", 0)]), None);
    }

    /// A login name that is not the address — some university servers.
    #[test]
    fn a_separate_login_name_is_used_when_the_account_has_one() {
        let mut r = row("first.last@uni.example", "smtp.uni.example", 587);
        r.username = Some("s1234567".into());
        assert_eq!(
            submission_for("first.last@uni.example", &[r]).unwrap().user,
            "s1234567"
        );
    }
}
