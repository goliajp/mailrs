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

use super::outcome::Outcome;

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
    /// Which provider registered it, for renewing an access token.
    pub provider: String,
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
        provider: row.provider.clone(),
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

/// The account's credential, opened.
///
/// **Not a string.** Two writers put two shapes under `ext:secret:*`
/// — a password as it was typed, and a JSON object for OAuth — and
/// this returned whatever was inside. An OAuth account's whole blob
/// went to `AUTH PLAIN` as a password, the provider refused it, and
/// the message came back saying the password was wrong for an account
/// whose tokens were fine.
pub(super) fn open_secret(
    kevy_url: &str,
    key: &Arc<mailrs_secretbox::Key>,
    id: &str,
) -> Option<ext::Credential> {
    let mut conn = kevy_client::Connection::connect(kevy_url).ok()?;
    let name = secret_name(&mut conn, id)?;
    let sealed = conn.get(&name).ok()??;
    let sealed = String::from_utf8(sealed).ok()?;
    credential_from_sealed(key, &sealed)
}

/// Open one sealed secret into the credential it holds.
///
/// Its own function because the rest of `open_secret` needs a kevy
/// connection, and this step — the one that decides whether the sender
/// authenticates with a password or a token — is where the defect was.
pub(super) fn credential_from_sealed(
    key: &Arc<mailrs_secretbox::Key>,
    sealed: &str,
) -> Option<ext::Credential> {
    let opened = mailrs_secretbox::open(key, sealed).ok()?;
    Some(ext::Credential::parse(&String::from_utf8(opened).ok()?))
}

/// Where this account's secret is stored.
///
/// It is keyed by user as well as id, and the sender matched on the
/// address alone — so the user is whichever key holds this id.
fn secret_name(conn: &mut kevy_client::Connection, id: &str) -> Option<Vec<u8>> {
    let names = conn.keys(b"ext:secret:*").ok()?;
    let suffix = format!(":{id}");
    names
        .into_iter()
        .find(|k| String::from_utf8_lossy(k).ends_with(&suffix))
}

/// The credential to authenticate with, renewed first when it is due.
///
/// A password is returned as it is. An access token is renewed
/// **before** it lapses rather than after being refused, because a
/// refusal at this point is indistinguishable from a wrong password:
/// the message is returned to the person, saying their account details
/// are wrong, for an account whose credentials are perfectly good.
///
/// A renewal that cannot reach the provider is transient — the message
/// waits and tries again. Only a refusal of the refresh token itself
/// is permanent, and it is the one case where a person really must
/// sign in again.
pub(super) async fn renewed_if_due(
    kevy_url: &str,
    key: &Arc<mailrs_secretbox::Key>,
    sub: &Submission,
    cred: ext::Credential,
) -> Result<ext::Credential, Outcome> {
    let ext::Credential::Oauth {
        access,
        refresh,
        expires_at,
    } = &cred
    else {
        return Ok(cred);
    };
    let now = now_secs();
    if !mailrs_oauth_client::needs_refresh(*expires_at, now) {
        return Ok(cred);
    }
    let Some(provider) = mailrs_oauth_client::mail_provider(&sub.provider) else {
        return Err(Outcome::Permanent(format!(
            "this server no longer has a registered application for {}, so this \
             account's access cannot be renewed",
            sub.provider
        )));
    };
    let client_secret = std::env::var(mailrs_fastcore::external_sync_secret::client_secret_var(
        &sub.provider,
    ))
    .unwrap_or_default();
    let fresh = match mailrs_oauth_client::renew(&provider, refresh, &client_secret, &sub.user, now)
        .await
    {
        Ok(f) => f,
        // Only a refused refresh token means signing in again. A
        // provider that cannot be reached is a reason to wait.
        Err(e @ mailrs_oauth_client::RenewError::Refused(_)) => {
            return Err(Outcome::Permanent(e.to_string()));
        }
        Err(e) => return Err(Outcome::Transient(e.to_string())),
    };

    // Seal it back, so the next message and the next sync use the new
    // one. A renewal that is not stored is a renewal on every send.
    if let Ok(mut conn) = kevy_client::Connection::connect(kevy_url)
        && let Some(name) = secret_name(&mut conn, &sub.account_id)
    {
        let payload = serde_json::json!({
            "access_token": fresh.access,
            "refresh_token": fresh.refresh,
            "expires_at": fresh.expires_at,
        })
        .to_string();
        if let Ok(sealed) = mailrs_secretbox::seal(key, payload.as_bytes()) {
            let _ = conn.set(&name, sealed.as_bytes());
        }
    }
    let _ = access;
    Ok(ext::Credential::Oauth {
        access: fresh.access,
        refresh: fresh.refresh,
        expires_at: fresh.expires_at,
    })
}

/// Seconds since the epoch.
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Which SMTP verb a credential authenticates with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Verb {
    /// `AUTH PLAIN` — a password, as RFC 4616 describes it.
    Plain,
    /// `AUTH XOAUTH2` — an access token, which `AUTH PLAIN` refuses.
    Xoauth2,
}

impl Verb {
    /// What to call the credential when the server turns it down.
    ///
    /// "your password is wrong" for an OAuth account sends a person
    /// looking for a password they never set.
    pub(super) fn what_was_refused(self) -> &'static str {
        match self {
            Self::Plain => "password",
            Self::Xoauth2 => "access token",
        }
    }
}

/// The verb this credential authenticates with.
pub(super) fn verb_for(cred: &ext::Credential) -> Verb {
    match cred {
        ext::Credential::Oauth { .. } => Verb::Xoauth2,
        ext::Credential::Password(_) => Verb::Plain,
    }
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

    /// The sealed shape decides the verb, and getting it wrong is not
    /// visible: an OAuth blob sent through `AUTH PLAIN` is refused,
    /// and a refusal at this point is returned to the person as "your
    /// password is wrong" for an account whose tokens are fine.
    #[test]
    fn an_oauth_account_is_told_apart_from_a_password_one() {
        let token = ext::Credential::parse(
            r#"{"access_token":"ya29.a","refresh_token":"1//r","expires_at":99}"#,
        );
        assert!(
            matches!(token, ext::Credential::Oauth { .. }),
            "a sealed token set read as a password: {token:?}"
        );
        let pw = ext::Credential::parse("hunter2");
        assert!(
            matches!(pw, ext::Credential::Password(_)),
            "a password read as a token set: {pw:?}"
        );
    }

    /// Which provider registered the account travels with it, because
    /// renewing an access token needs to know whose token it is.
    #[test]
    fn the_provider_travels_with_the_submission() {
        let mut r = row("me@gmail.com", "smtp.gmail.com", 465);
        r.provider = "google".into();
        let sub = submission_for("me@gmail.com", &[r]).expect("a connected address");
        assert_eq!(sub.provider, "google");
    }

    /// A password is never renewed — there is nothing to renew it
    /// against, and a needless round trip to a provider would fail
    /// every send for an account that works.
    #[tokio::test]
    async fn a_password_account_is_never_renewed() {
        let key = Arc::new(mailrs_secretbox::Key::from_passphrase("k"));
        let mut r = row("me@gmail.com", "smtp.gmail.com", 465);
        r.provider = "google".into();
        let sub = submission_for("me@gmail.com", &[r]).expect("a connected address");
        let out = renewed_if_due("", &key, &sub, ext::Credential::Password("p".into())).await;
        assert!(
            matches!(out, Ok(ext::Credential::Password(ref p)) if p == "p"),
            "a password went looking for a provider"
        );
    }

    /// A token still good is used as it is. Renewing on every send
    /// would put a provider round trip in front of every message.
    #[tokio::test]
    async fn a_token_that_has_not_lapsed_is_used_as_it_is() {
        let key = Arc::new(mailrs_secretbox::Key::from_passphrase("k"));
        let mut r = row("me@gmail.com", "smtp.gmail.com", 465);
        r.provider = "google".into();
        let sub = submission_for("me@gmail.com", &[r]).expect("a connected address");
        let far_off = now_secs() + 86_400;
        let out = renewed_if_due(
            "",
            &key,
            &sub,
            ext::Credential::Oauth {
                access: "ya29.good".into(),
                refresh: "1//r".into(),
                expires_at: far_off,
            },
        )
        .await;
        assert!(
            matches!(out, Ok(ext::Credential::Oauth { ref access, .. }) if access == "ya29.good"),
            "a token that had not lapsed was renewed anyway"
        );
    }

    /// The step the defect was in: what comes out of the store decides
    /// the verb. Reading every sealed secret as a password sent an
    /// OAuth blob through `AUTH PLAIN`, and the refusal came back to
    /// the person as a wrong password.
    #[test]
    fn a_sealed_token_set_opens_as_a_token_set() {
        let key = Arc::new(mailrs_secretbox::Key::from_passphrase("k"));
        let sealed = mailrs_secretbox::seal(
            &key,
            br#"{"access_token":"ya29.a","refresh_token":"1//r","expires_at":9}"#,
        )
        .expect("seal");
        let cred = credential_from_sealed(&key, &sealed).expect("open");
        assert_eq!(
            verb_for(&cred),
            Verb::Xoauth2,
            "an OAuth account would authenticate as if the JSON were a password"
        );
        assert_eq!(cred.secret(), "ya29.a", "the whole blob went as the token");
    }

    #[test]
    fn a_sealed_password_opens_as_a_password() {
        let key = Arc::new(mailrs_secretbox::Key::from_passphrase("k"));
        let sealed = mailrs_secretbox::seal(&key, b"hunter2").expect("seal");
        let cred = credential_from_sealed(&key, &sealed).expect("open");
        assert_eq!(verb_for(&cred), Verb::Plain);
        assert_eq!(cred.secret(), "hunter2");
    }

    /// The words a person reads when the server says no.
    #[test]
    fn a_refused_token_is_not_called_a_password() {
        assert_eq!(Verb::Xoauth2.what_was_refused(), "access token");
        assert_eq!(Verb::Plain.what_was_refused(), "password");
    }

    /// Pausing stops the reading, not the sending.
    ///
    /// The credential is still held and still valid; refusing to send
    /// from an address somebody owns would be a second meaning nobody
    /// asked for. Said in three comments and, until now, guarded
    /// nowhere.
    #[test]
    fn a_paused_account_can_still_send() {
        let mut r = row("me@gmail.com", "smtp.gmail.com", 465);
        r.state = ext::State::Paused;
        assert!(
            submission_for("me@gmail.com", &[r]).is_some(),
            "pausing the reading also stopped the sending"
        );
    }

    /// And the one that genuinely cannot: some providers count failed
    /// attempts and lock the account, so failing at home beats failing
    /// at theirs.
    #[test]
    fn a_refused_credential_does_not_reach_the_provider() {
        let mut r = row("me@gmail.com", "smtp.gmail.com", 465);
        r.state = ext::State::NeedsAuth;
        assert!(submission_for("me@gmail.com", &[r]).is_none());
    }
}
