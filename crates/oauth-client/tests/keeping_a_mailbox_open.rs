//! Staying signed in to somebody else's mailbox.
//!
//! Signing a person in is one round trip. Reading their mail for the
//! next year is a token that expires every hour and a refresh token
//! that renews it — which is why these live beside the login flow
//! rather than inside it.

use mailrs_oauth_client::{
    IdentitySource, Provider, TokenResponse, mailbox_scopes, needs_refresh, parse_token_response,
    refresh_request_body,
};

fn google() -> Provider {
    Provider {
        key: "google".into(),
        issuer: "https://accounts.google.com".into(),
        authorize_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
        token_url: "https://oauth2.googleapis.com/token".into(),
        userinfo_url: None,
        scopes: vec!["openid".into()],
        client_id: "cid".into(),
        redirect_uri: "https://mail.golia.jp/oauth/callback".into(),
        source: IdentitySource::IdToken,
        require_verified_email: true,
    }
}

/// Without `offline_access` (or Google's `access_type=offline`) the
/// provider returns **no refresh token at all** — the account works
/// for one hour and then asks to sign in again, and nothing in the
/// flow says why.
#[test]
fn the_scopes_ask_for_a_refresh_token() {
    let s = mailbox_scopes("google");
    assert!(s.contains("https://mail.google.com/"), "{s}");
    let m = mailbox_scopes("microsoft");
    assert!(m.contains("IMAP.AccessAsUser.All"), "{m}");
    assert!(m.contains("SMTP.Send"), "{m}");
    assert!(m.contains("offline_access"), "{m}");
}

#[test]
fn an_unknown_provider_has_no_mailbox_scopes() {
    assert!(mailbox_scopes("github").is_empty());
}

#[test]
fn a_refresh_body_asks_for_the_right_grant() {
    let body = refresh_request_body(&google(), "1//refresh", "secret");
    assert!(body.contains("grant_type=refresh_token"), "{body}");
    assert!(body.contains("refresh_token=1%2F%2Frefresh"), "{body}");
    assert!(body.contains("client_id=cid"), "{body}");
}

/// A token answer for a mailbox carries two fields a login answer
/// never had. Both optional, so no existing caller changes.
#[test]
fn a_token_answer_carries_the_refresh_token_and_its_life() {
    let body = br#"{"access_token":"ya29.a0","refresh_token":"1//0g","expires_in":3599}"#;
    let t: TokenResponse = parse_token_response(&google(), body).expect("parsed");
    assert_eq!(t.access_token, "ya29.a0");
    assert_eq!(t.refresh_token.as_deref(), Some("1//0g"));
    assert_eq!(t.expires_in, Some(3599));
}

/// A refresh answer usually omits the refresh token, which means keep
/// the one already held. Reading that as "the account has no refresh
/// token" logs somebody out an hour later.
#[test]
fn a_refresh_answer_without_a_new_refresh_token_is_not_a_lost_one() {
    let body = br#"{"access_token":"ya29.b1","expires_in":3599}"#;
    let t: TokenResponse = parse_token_response(&google(), body).expect("parsed");
    assert_eq!(t.refresh_token, None);
}

/// The renewal has to happen **before** the token expires.
///
/// A worker that discovers expiry by being refused marks the account
/// NeedsAuth, and the person is told to sign in again for a token that
/// could have been renewed without them.
#[test]
fn a_token_is_renewed_before_it_expires_not_after_it_fails() {
    let expires_at = 10_000;
    assert!(!needs_refresh(expires_at, 9_000), "renewed far too early");
    assert!(
        needs_refresh(expires_at, 9_950),
        "waited until it was refused"
    );
    assert!(needs_refresh(expires_at, 10_500), "an expired token is due");
}

/// An account with no expiry recorded is due: it is either from before
/// this existed or was written by something that did not say, and
/// asking once is cheaper than a mailbox that quietly stops.
#[test]
fn an_unknown_expiry_is_due_rather_than_assumed_fresh() {
    assert!(needs_refresh(0, 1));
}
