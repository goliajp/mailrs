//! Connecting a mailbox that will not take a password.
//!
//! Google and Microsoft both refuse a password from a mail client, so
//! OAuth is not an alternative route to those mailboxes — it is the
//! only one. What can be asserted without a registered application is
//! everything up to the redirect, and that is where the mistakes are.

use mailrs_mailprovider::AuthKind;

/// Both are `oauth2` in the provider table, which is what makes the
/// set-up form hide the password field for them. If this ever read
/// `password`, somebody would be asked for one that cannot work.
#[test]
fn neither_provider_is_offered_a_password_field() {
    for addr in [
        "someone@gmail.com",
        "someone@outlook.com",
        "someone@hotmail.co.jp",
    ] {
        let p = mailrs_mailprovider::preset_for(addr).expect("known");
        assert_eq!(p.auth, AuthKind::OAuth2, "{addr}");
        assert!(p.secret_help.is_none(), "{addr} offers a secret to type");
    }
}

/// Without `offline_access` the provider returns no refresh token, and
/// the account works for one hour before asking to sign in again with
/// nothing saying why.
#[test]
fn the_scopes_ask_for_long_lived_access() {
    let m = mailrs_oauth_client::mailbox_scopes("microsoft");
    assert!(m.contains("offline_access"), "{m}");
    assert!(m.contains("IMAP.AccessAsUser.All"), "{m}");
    let g = mailrs_oauth_client::mailbox_scopes("google");
    assert!(g.contains("https://mail.google.com/"), "{g}");
}

/// A deployment that has not registered an application cannot connect
/// these mailboxes, and every route says so rather than starting a
/// flow that cannot finish.
#[test]
fn an_unregistered_deployment_has_no_provider() {
    // Nothing is set in the test environment, which is the same state
    // a fresh deployment is in.
    assert!(mailrs_oauth_client::mail_provider("google").is_none());
    assert!(mailrs_oauth_client::mail_provider("microsoft").is_none());
    // And a provider nobody has heard of is not one either.
    assert!(mailrs_oauth_client::mail_provider("yahoo").is_none());
}

/// The refusal has to say what to do, not just that it failed: these
/// providers not taking a password is the reason OAuth exists here,
/// and somebody reading the message should not have to guess that.
///
/// Asserted on the message the route actually produces. An earlier
/// version read the source file for the words, and failed because a
/// string continuation had split them across two lines — a test
/// arguing with the formatter rather than checking behaviour.
#[test]
fn the_refusal_explains_itself() {
    let why = mailrs_webapi::handlers::account_oauth::why_not_registered("google");
    assert!(why.contains("has not registered"), "{why}");
    assert!(why.contains("does not accept a password"), "{why}");
    assert!(why.contains("google"), "{why}");
}
