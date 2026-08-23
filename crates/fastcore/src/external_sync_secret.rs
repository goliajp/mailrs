//! An external account's secret: what was sealed, opening it,
//! renewing it before it lapses, and sealing it back.
//!
//! Split from `external_sync.rs` at the file-size gate along a seam
//! that was already there — nothing here speaks IMAP, and the sync
//! loop does not know how a secret is stored.

use std::sync::Arc;

use mailrs_core_sidestate::families::external_accounts::AccountRow;

use crate::FastcoreState;
use crate::external_sync::now_secs;

/// The deployment's sealing key, if one is configured.
pub(crate) fn sealing_key() -> Option<mailrs_secretbox::Key> {
    std::env::var("MAILRS_ACCOUNT_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| mailrs_secretbox::Key::from_passphrase(&v))
}

/// What was sealed for an account.
///
/// Two writers put two shapes under `ext:secret:*` — a password as it
/// was typed, and a JSON object for OAuth — and one reader returned
/// whatever was inside as a string. An OAuth account therefore handed
/// `{"access_token":…}` to `AUTHENTICATE XOAUTH2` as the token, the
/// provider refused it, and the worker read that as a refused
/// credential and asked the person to sign in again for an account
/// whose tokens were perfectly good.
///
/// Nothing errored; both sides were self-consistent. One shape, read
/// once, is the fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Credential {
    /// A password or an app password, as typed.
    Password(String),
    /// OAuth, with the instant its access token stops working.
    Oauth {
        /// What `AUTHENTICATE XOAUTH2` is given.
        access: String,
        /// What renews it.
        refresh: String,
        /// Epoch seconds. Absolute, because a stored duration means
        /// nothing an hour after it was written.
        expires_at: i64,
    },
}

impl Credential {
    /// Read a sealed value.
    ///
    /// Anything that is not the OAuth object **is** a password — which
    /// is what every value written before OAuth existed is, and what
    /// every app-password account still writes.
    pub(crate) fn parse(raw: &str) -> Self {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
            return Self::Password(raw.to_string());
        };
        let (Some(access), Some(refresh)) = (
            v.get("access_token").and_then(|x| x.as_str()),
            v.get("refresh_token").and_then(|x| x.as_str()),
        ) else {
            return Self::Password(raw.to_string());
        };
        Self::Oauth {
            access: access.to_string(),
            refresh: refresh.to_string(),
            expires_at: v.get("expires_at").and_then(|x| x.as_i64()).unwrap_or(0),
        }
    }

    /// What to send as the secret, right now.
    ///
    /// For OAuth this is the access token — renewing it when due is
    /// the caller's job, and it has to happen **before** connecting:
    /// discovering expiry by being refused marks the account NeedsAuth
    /// and asks a person to re-authenticate something that could have
    /// been renewed without them.
    pub(crate) fn as_secret(&self) -> &str {
        match self {
            Self::Password(p) => p,
            Self::Oauth { access, .. } => access,
        }
    }
}

/// The account's credential, opened.
fn open_secret(state: &Arc<FastcoreState>, user: &str, id: &str) -> Result<Credential, String> {
    let key = sealing_key().ok_or("MAILRS_ACCOUNT_KEY is not set on this server")?;
    let mut conn = state
        .net_conn()
        .ok_or("the side-state store is unreachable")?;
    let sealed = conn
        .get(format!("ext:secret:{user}:{id}").as_bytes())
        .map_err(|e| e.to_string())?
        .ok_or("no stored password for this account")?;
    let sealed = String::from_utf8(sealed).map_err(|_| "the stored password is not text")?;
    let opened = mailrs_secretbox::open(&key, &sealed).map_err(|e| e.to_string())?;
    let raw = String::from_utf8(opened).map_err(|_| "the stored password is not text")?;
    Ok(Credential::parse(&raw))
}

/// The secret to connect with, renewing it first when it is due.
///
/// **Before connecting, not after being refused.** A worker that
/// discovers expiry by being turned away marks the account NeedsAuth,
/// and a person is asked to sign in again for a token that could have
/// been renewed without them ever knowing it had lapsed.
///
/// A renewal that fails is not the same as a credential that was
/// refused: the provider may simply be unreachable, and marking the
/// account broken for that is something only a person can undo. Only a
/// refusal of the *refresh token itself* means signing in again.
pub(crate) async fn usable_secret(
    state: &Arc<FastcoreState>,
    user: &str,
    row: &AccountRow,
) -> Result<String, String> {
    let cred = open_secret(state, user, &row.id)?;
    let Credential::Oauth {
        access,
        refresh,
        expires_at,
    } = &cred
    else {
        return Ok(cred.as_secret().to_string());
    };
    if !mailrs_oauth_client::needs_refresh(*expires_at, now_secs()) {
        return Ok(access.clone());
    }

    let Some(provider) = mailrs_oauth_client::mail_provider(&row.provider) else {
        // The account was connected when an application was
        // registered and the registration has since gone. Say that,
        // rather than letting it read as a wrong password.
        return Err(format!(
            "this server no longer has a registered application for {}, so this \
             account's access cannot be renewed",
            row.provider
        ));
    };
    let client_secret = std::env::var(client_secret_var(&row.provider)).unwrap_or_default();

    // The exchange itself is shared with the sender, which renews the
    // same credentials from a different store. Two copies of it would
    // be two places for the keep-the-old-refresh-token rule and the
    // `invalid_grant` reading to drift apart.
    let fresh =
        mailrs_oauth_client::renew(&provider, refresh, &client_secret, &row.email, now_secs())
            .await
            .map_err(|e| e.to_string())?;
    reseal(
        state,
        user,
        &row.id,
        &fresh.access,
        &fresh.refresh,
        fresh.expires_at,
    )?;
    Ok(fresh.access)
}

/// Which environment variable holds a provider's client secret.
///
/// Shared with the sender so the two do not renew against different
/// registrations.
pub fn client_secret_var(provider: &str) -> &'static str {
    match provider {
        "google" => "MAILRS_GOOGLE_MAIL_CLIENT_SECRET",
        _ => "MAILRS_MICROSOFT_MAIL_CLIENT_SECRET",
    }
}

/// Store a renewed token set, sealed.
///
/// Overwrites in place under the same key: an account has one
/// credential, and a renewal is that credential's new value rather
/// than a second one. Writing it beside the old would leave two, and
/// two is how a rotation comes to be half-done.
fn reseal(
    state: &Arc<FastcoreState>,
    user: &str,
    id: &str,
    access: &str,
    refresh: &str,
    expires_at: i64,
) -> Result<(), String> {
    let key = sealing_key().ok_or("MAILRS_ACCOUNT_KEY is not set on this server")?;
    let sealed = mailrs_secretbox::seal(
        &key,
        serde_json::json!({
            "access_token": access,
            "refresh_token": refresh,
            "expires_at": expires_at,
        })
        .to_string()
        .as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    let mut conn = state
        .net_conn()
        .ok_or("the side-state store is unreachable")?;
    // Only over a secret that is still there. Disconnecting deletes
    // the row and its sealed credential; a renewal that finished after
    // that would write the credential back on its own, leaving a
    // sealed token set for an account nobody can see and nobody will
    // delete.
    let secret_key = format!("ext:secret:{user}:{id}");
    match conn.get(secret_key.as_bytes()) {
        Ok(Some(_)) => {}
        _ => return Err("this account was disconnected while it was syncing".into()),
    }
    conn.set(secret_key.as_bytes(), sealed.as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Put a line on the account row about what it is doing.
///
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sealed_password_and_a_sealed_token_set_are_told_apart() {
        let pw = Credential::parse("hunter2-and-then-some");
        assert_eq!(pw, Credential::Password("hunter2-and-then-some".into()));
        assert_eq!(pw.as_secret(), "hunter2-and-then-some");

        let oauth = Credential::parse(
            r#"{"access_token":"ya29.a0","refresh_token":"1//0g","expires_at":1787}"#,
        );
        assert_eq!(
            oauth,
            Credential::Oauth {
                access: "ya29.a0".into(),
                refresh: "1//0g".into(),
                expires_at: 1787,
            }
        );
        // The token, not the document that holds it.
        assert_eq!(oauth.as_secret(), "ya29.a0");
    }

    #[test]
    fn a_password_that_happens_to_be_json_is_still_a_password() {
        for pw in [
            r#"{"a":1}"#,
            "[1,2,3]",
            "null",
            r#"{"access_token":"only-one"}"#,
        ] {
            assert!(
                matches!(Credential::parse(pw), Credential::Password(_)),
                "{pw} was read as a token set"
            );
        }
    }

    #[test]
    fn a_secret_from_before_oauth_existed_still_opens() {
        assert_eq!(
            Credential::parse("授权码-abcd efgh"),
            Credential::Password("授权码-abcd efgh".into())
        );
    }

    #[test]
    fn a_token_is_due_before_it_lapses() {
        let expires_at = 10_000;
        assert!(!mailrs_oauth_client::needs_refresh(expires_at, 9_000));
        assert!(mailrs_oauth_client::needs_refresh(expires_at, 9_800));
        assert!(mailrs_oauth_client::needs_refresh(expires_at, 10_001));
    }

    #[test]
    fn a_password_account_is_never_due_for_renewal() {
        let cred = Credential::parse("an-app-password");
        assert!(matches!(cred, Credential::Password(_)));
        assert_eq!(cred.as_secret(), "an-app-password");
    }

    #[test]
    fn only_a_refused_refresh_token_asks_for_a_new_sign_in() {
        use mailrs_core_sidestate::families::external_accounts as x;
        let row = x::AccountRow::default();
        let refused = x::with_failure(row.clone(), 1_000, "invalid_grant renewing me@gmail.com");
        assert_eq!(refused.state, x::State::NeedsAuth);

        let unreachable = x::with_failure(row, 1_000, "could not reach google to renew: timed out");
        assert_eq!(unreachable.state, x::State::Error);
    }
}
