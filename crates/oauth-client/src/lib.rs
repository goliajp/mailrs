//! Signing in with somebody else's identity provider.
//!
//! Four things every provider needs and none of them agree on: where to send
//! the browser, what a token exchange looks like, where the person's identity
//! is, and which parts of it can be believed. This crate holds the parts that
//! are the same and makes the differences data rather than branches.
//!
//! Everything here is pure. The two HTTP calls a real flow needs — token
//! exchange and, for providers without an `id_token`, a userinfo fetch — take
//! their responses as bytes, so the URL building, the PKCE derivation and the
//! claim reading are all testable without a network.
//!
//! **The security question this crate answers is "who is this", not "whose
//! mailbox is this".** Those are different, and conflating them is account
//! takeover: an identity provider says what account *it* authenticated, and
//! an email claim is that provider's opinion about a string. What the caller
//! does with an [`ExternalIdentity`] — link it, require a password first,
//! refuse it — is a policy decision that belongs above this layer. See
//! `.claude/rfcs/20260801-external-login.md`.

#![deny(missing_docs)]

use serde::{Deserialize, Serialize};

/// What went wrong reading a provider's answer.
#[derive(Debug, thiserror::Error)]
pub enum OauthError {
    /// The response was not JSON, or not the JSON expected.
    #[error("malformed response from {provider}: {detail}")]
    Malformed {
        /// Which provider.
        provider: String,
        /// What was wrong with it.
        detail: String,
    },
    /// The provider returned an OAuth error response.
    #[error("{provider} refused: {error}")]
    Refused {
        /// Which provider.
        provider: String,
        /// The `error` field, verbatim.
        error: String,
    },
    /// The identity is unusable — no stable subject, or no verified email
    /// where one is required.
    #[error("{provider} gave no usable identity: {detail}")]
    NoIdentity {
        /// Which provider.
        provider: String,
        /// Why not.
        detail: String,
    },
}

/// Where a provider's identity lives once the token exchange is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentitySource {
    /// Claims inside the `id_token` — the OIDC case.
    IdToken,
    /// A separate call. GitHub has no `id_token` at all; its identity is
    /// `GET /user` plus `GET /user/emails`, and the second call exists
    /// because the first returns a *public profile* email that may be blank,
    /// stale, or never verified.
    UserinfoThenEmails,
}

/// One configured provider.
///
/// A struct rather than an enum with per-variant code: adding a provider
/// should be adding a row, and a `match` over providers is where the fifth
/// one gets forgotten in a branch somebody did not update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    /// Stable key — `portal`, `google`, `github`, …. Part of the stored link,
    /// so it must not change once a link exists.
    pub key: String,
    /// The `iss` this provider is trusted to assert.
    ///
    /// Pinned in configuration and compared against the token, never taken
    /// from it. Microsoft's issuer is tenant-specific, so accepting whatever
    /// arrives would accept any tenant — including one the attacker owns.
    pub issuer: String,
    /// Where the browser goes.
    pub authorize_url: String,
    /// Where the code is exchanged.
    pub token_url: String,
    /// Where the identity is read from, when it is not in an `id_token`.
    pub userinfo_url: Option<String>,
    /// Scopes, space-joined into the request.
    pub scopes: Vec<String>,
    /// This deployment's client id.
    pub client_id: String,
    /// Where the provider sends the browser back.
    pub redirect_uri: String,
    /// How to read the identity out of the exchange.
    pub source: IdentitySource,
    /// Whether an unverified email disqualifies the identity.
    ///
    /// True everywhere it can be: an unverified address is a string the user
    /// typed, and treating it as identity is how a link gets attached to
    /// somebody else's mailbox.
    pub require_verified_email: bool,
}

/// A person, as one provider describes them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalIdentity {
    /// The issuer that asserted this.
    pub issuer: String,
    /// The provider's stable identifier for the person.
    ///
    /// The half of the link that is allowed to be permanent. Email changes;
    /// this does not, which is why the link is keyed on it.
    pub subject: String,
    /// The address the provider associates with them, if any.
    ///
    /// Present for display and for the narrow auto-provision path. **Never a
    /// credential on its own.**
    pub email: Option<String>,
    /// Whether the provider says it verified that address.
    pub email_verified: bool,
    /// A name to show, if the provider offered one.
    pub display_name: Option<String>,
}

/// A PKCE challenge pair.
///
/// The verifier stays on the server and the challenge goes in the URL, so an
/// authorization code intercepted in the redirect cannot be exchanged by
/// whoever intercepted it. portal requires this; everyone else should have it
/// anyway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkce {
    /// Held server-side until the exchange.
    pub verifier: String,
    /// Sent in the authorization request.
    pub challenge: String,
}

impl Pkce {
    /// Derive the S256 challenge for a verifier.
    ///
    /// RFC 7636 §4.2: base64url of the SHA-256, no padding. Padding is the
    /// part implementations get wrong, and a `=` in the URL is a challenge
    /// the provider will not match.
    pub fn from_verifier(verifier: impl Into<String>) -> Self {
        use base64::Engine as _;
        use sha2::Digest as _;
        let verifier = verifier.into();
        let digest = sha2::Sha256::digest(verifier.as_bytes());
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
        Self {
            verifier,
            challenge,
        }
    }

    /// A fresh verifier and its challenge.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
        use base64::Engine as _;
        Self::from_verifier(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
    }
}

/// Percent-encode everything that is not unreserved (RFC 3986 §2.3).
///
/// Written out rather than pulled in: a redirect URI containing `&` or `=`
/// that is not encoded silently changes which parameters the provider sees,
/// and the whole point of the redirect URI is that the provider matches it.
pub(crate) fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The URL to send the browser to.
pub fn authorize_url(p: &Provider, state: &str, pkce: &Pkce) -> String {
    let scope = p.scopes.join(" ");
    format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}\
         &code_challenge={}&code_challenge_method=S256",
        p.authorize_url,
        encode(&p.client_id),
        encode(&p.redirect_uri),
        encode(&scope),
        encode(state),
        encode(&pkce.challenge),
    )
}

/// The form body for the token exchange.
pub fn token_request_body(p: &Provider, code: &str, pkce: &Pkce, client_secret: &str) -> String {
    format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}\
         &client_secret={}&code_verifier={}",
        encode(code),
        encode(&p.redirect_uri),
        encode(&p.client_id),
        encode(client_secret),
        encode(&pkce.verifier),
    )
}

/// The `access_token` and, when the provider is an OIDC one, the `id_token`.
pub fn parse_token_response(p: &Provider, body: &[u8]) -> Result<TokenResponse, OauthError> {
    let v: serde_json::Value = serde_json::from_slice(body).map_err(|e| OauthError::Malformed {
        provider: p.key.clone(),
        detail: e.to_string(),
    })?;
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(OauthError::Refused {
            provider: p.key.clone(),
            error: err.to_string(),
        });
    }
    let access_token = v
        .get("access_token")
        .and_then(|t| t.as_str())
        .ok_or_else(|| OauthError::Malformed {
            provider: p.key.clone(),
            detail: "no access_token".into(),
        })?
        .to_string();
    Ok(TokenResponse {
        access_token,
        id_token: v
            .get("id_token")
            .and_then(|t| t.as_str())
            .map(str::to_string),
        refresh_token: v
            .get("refresh_token")
            .and_then(|t| t.as_str())
            .map(str::to_string),
        expires_in: v.get("expires_in").and_then(|t| t.as_i64()),
    })
}

/// What a token exchange returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenResponse {
    /// For calling userinfo, and for IMAP's `AUTHENTICATE XOAUTH2`.
    pub access_token: String,
    /// Present on OIDC providers.
    pub id_token: Option<String>,
    /// The long-lived half, present on the **first** exchange when the
    /// scopes asked for it.
    ///
    /// A refresh answer usually omits it, and that means *keep the one
    /// already held*. Reading its absence as "this account has no
    /// refresh token" logs somebody out an hour later.
    pub refresh_token: Option<String>,
    /// Seconds the access token is good for, from the moment of the
    /// answer. Absent from some providers' refresh answers, which is
    /// why a caller stores an absolute instant rather than this.
    pub expires_in: Option<i64>,
}

/// The claims payload of a JWT, undecoded and **unverified**.
///
/// Signature verification belongs to the caller, which is the one holding the
/// provider's JWKS. Returning the claims without checking them is only safe
/// because [`identity_from_id_token`] refuses to build an identity from an
/// issuer that does not match the pinned one — but a caller doing anything
/// else with this must verify first.
pub fn jwt_claims_unverified(token: &str) -> Result<serde_json::Value, OauthError> {
    use base64::Engine as _;
    let mut parts = token.split('.');
    let (_header, payload) = (parts.next(), parts.next());
    let payload = payload.ok_or_else(|| OauthError::Malformed {
        provider: "jwt".into(),
        detail: "not three dot-separated parts".into(),
    })?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|e| OauthError::Malformed {
            provider: "jwt".into(),
            detail: format!("payload is not base64url: {e}"),
        })?;
    serde_json::from_slice(&bytes).map_err(|e| OauthError::Malformed {
        provider: "jwt".into(),
        detail: e.to_string(),
    })
}

/// Build an identity from OIDC claims.
///
/// The issuer is compared against the pinned one and the claims are refused
/// if it differs. That check is the reason a tenant-specific provider like
/// Microsoft is safe to configure at all: without it, a token from any tenant
/// would be accepted as this one.
pub fn identity_from_claims(
    p: &Provider,
    claims: &serde_json::Value,
) -> Result<ExternalIdentity, OauthError> {
    let iss = claims.get("iss").and_then(|v| v.as_str()).unwrap_or("");
    if iss != p.issuer {
        return Err(OauthError::NoIdentity {
            provider: p.key.clone(),
            detail: format!("issuer {iss:?} is not the configured {:?}", p.issuer),
        });
    }
    let subject = claims
        .get("sub")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| OauthError::NoIdentity {
            provider: p.key.clone(),
            detail: "no sub claim".into(),
        })?
        .to_string();
    let email = claims
        .get("email")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    // Absent means unverified. A provider that does not say has not said yes.
    let email_verified = claims
        .get("email_verified")
        .and_then(as_bool_loose)
        .unwrap_or(false);
    if p.require_verified_email && email.is_some() && !email_verified {
        return Err(OauthError::NoIdentity {
            provider: p.key.clone(),
            detail: "email present but not verified".into(),
        });
    }
    Ok(ExternalIdentity {
        issuer: p.issuer.clone(),
        subject,
        email,
        email_verified,
        display_name: claims
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    })
}

/// `email_verified` arrives as a bool from most providers and as the string
/// `"true"` from some. Both mean the same thing and neither should be read as
/// the other's falsehood.
fn as_bool_loose(v: &serde_json::Value) -> Option<bool> {
    match v {
        serde_json::Value::Bool(b) => Some(*b),
        serde_json::Value::String(s) => match s.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Build an identity from GitHub's two calls.
///
/// `user` is `GET /user` and `emails` is `GET /user/emails`. The second is
/// necessary and not a nicety: `user.email` is the *public profile* address,
/// which the account holder types in freely and GitHub never verifies. Only
/// an entry in `/user/emails` with `verified: true` is a statement GitHub is
/// making rather than repeating.
pub fn identity_from_github(
    p: &Provider,
    user: &serde_json::Value,
    emails: &serde_json::Value,
) -> Result<ExternalIdentity, OauthError> {
    let subject = user
        .get("id")
        .and_then(|v| v.as_i64())
        .map(|n| n.to_string())
        .ok_or_else(|| OauthError::NoIdentity {
            provider: p.key.clone(),
            detail: "no numeric id".into(),
        })?;
    let list = emails.as_array().map(Vec::as_slice).unwrap_or(&[]);
    // Primary and verified, else any verified. An unverified address is not
    // a fallback — it is the thing being excluded.
    let pick = list
        .iter()
        .find(|e| {
            e.get("verified").and_then(|v| v.as_bool()).unwrap_or(false)
                && e.get("primary").and_then(|v| v.as_bool()).unwrap_or(false)
        })
        .or_else(|| {
            list.iter()
                .find(|e| e.get("verified").and_then(|v| v.as_bool()).unwrap_or(false))
        });
    let email = pick
        .and_then(|e| e.get("email"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    if p.require_verified_email && email.is_none() {
        return Err(OauthError::NoIdentity {
            provider: p.key.clone(),
            detail: "no verified email on the account".into(),
        });
    }
    Ok(ExternalIdentity {
        issuer: p.issuer.clone(),
        subject,
        email,
        email_verified: true,
        display_name: user
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| {
                user.get("login")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            }),
    })
}

pub mod mail;
pub use mail::*;

#[cfg(feature = "net")]
pub mod renew;
#[cfg(feature = "net")]
pub use renew::{RenewError, Renewed, renew};

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(key: &str, source: IdentitySource) -> Provider {
        Provider {
            key: key.into(),
            issuer: "https://portal.golia.jp".into(),
            authorize_url: "https://portal.golia.jp/oauth/authorize".into(),
            token_url: "https://portal.golia.jp/oauth/token".into(),
            userinfo_url: Some("https://portal.golia.jp/oauth/userinfo".into()),
            scopes: vec!["openid".into(), "email".into(), "profile".into()],
            client_id: "mailrs".into(),
            redirect_uri: "https://mail.golia.jp/api/auth/oidc/callback".into(),
            source,
            require_verified_email: true,
        }
    }

    /// RFC 7636 §4.2's own worked example. Padding is what implementations
    /// get wrong, and a `=` in the URL is a challenge the provider will not
    /// match.
    #[test]
    fn pkce_matches_the_rfc_example() {
        let p = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        assert_eq!(p.challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert!(!p.challenge.contains('='));
    }

    #[test]
    fn a_generated_verifier_derives_its_own_challenge() {
        let a = Pkce::generate();
        assert_eq!(
            Pkce::from_verifier(a.verifier.clone()).challenge,
            a.challenge
        );
        // Two generations must not collide, or the verifier proves nothing.
        assert_ne!(Pkce::generate().verifier, Pkce::generate().verifier);
    }

    /// A redirect URI carries `:` and `/`, and portal matches it exactly.
    /// Leaving those raw would also let a `&` in any parameter invent one.
    #[test]
    fn the_authorize_url_encodes_its_parameters() {
        let url = authorize_url(
            &provider("portal", IdentitySource::IdToken),
            "st&ate",
            &Pkce::from_verifier("v"),
        );
        assert!(
            url.contains("redirect_uri=https%3A%2F%2Fmail.golia.jp%2Fapi%2Fauth%2Foidc%2Fcallback")
        );
        assert!(url.contains("scope=openid%20email%20profile"));
        // The ampersand in the state does not become a parameter separator.
        assert!(url.contains("state=st%26ate"));
        assert!(url.contains("code_challenge_method=S256"));
    }

    /// Microsoft's issuer is tenant-specific. Accepting whatever the token
    /// says would accept any tenant, including one the attacker controls.
    #[test]
    fn claims_from_the_wrong_issuer_are_refused() {
        let p = provider("portal", IdentitySource::IdToken);
        let claims = serde_json::json!({
            "iss": "https://evil.example.com",
            "sub": "abc",
            "email": "lihao@golia.jp",
            "email_verified": true,
        });
        let err = identity_from_claims(&p, &claims).expect_err("wrong issuer");
        assert!(matches!(err, OauthError::NoIdentity { .. }));
    }

    #[test]
    fn a_good_token_yields_the_subject_and_email() {
        let p = provider("portal", IdentitySource::IdToken);
        let claims = serde_json::json!({
            "iss": "https://portal.golia.jp",
            "sub": "user-42",
            "email": "lihao@golia.jp",
            "email_verified": true,
            "name": "Lihao",
        });
        let id = identity_from_claims(&p, &claims).expect("identity");
        assert_eq!(id.subject, "user-42");
        assert_eq!(id.email.as_deref(), Some("lihao@golia.jp"));
        assert!(id.email_verified);
        assert_eq!(id.display_name.as_deref(), Some("Lihao"));
    }

    /// A provider that does not say has not said yes.
    #[test]
    fn an_absent_email_verified_is_not_a_yes() {
        let p = provider("portal", IdentitySource::IdToken);
        let claims = serde_json::json!({
            "iss": "https://portal.golia.jp",
            "sub": "user-42",
            "email": "lihao@golia.jp",
        });
        assert!(identity_from_claims(&p, &claims).is_err());
    }

    /// Some providers send the string. It means the same thing.
    #[test]
    fn email_verified_as_a_string_counts() {
        let p = provider("portal", IdentitySource::IdToken);
        let claims = serde_json::json!({
            "iss": "https://portal.golia.jp",
            "sub": "s",
            "email": "a@b.com",
            "email_verified": "true",
        });
        assert!(
            identity_from_claims(&p, &claims)
                .expect("identity")
                .email_verified
        );
    }

    /// No `sub` is no identity — there is nothing stable to key a link on.
    #[test]
    fn an_identity_without_a_subject_is_refused() {
        let p = provider("portal", IdentitySource::IdToken);
        let claims = serde_json::json!({ "iss": "https://portal.golia.jp", "email": "a@b.com" });
        assert!(identity_from_claims(&p, &claims).is_err());
    }

    /// GitHub's `user.email` is the *public profile* address — typed freely
    /// by the account holder and never verified. Reading it as identity is
    /// how a link gets attached to somebody else's mailbox.
    #[test]
    fn github_ignores_the_unverified_profile_email() {
        let p = provider("github", IdentitySource::UserinfoThenEmails);
        let user =
            serde_json::json!({ "id": 12345, "login": "someone", "email": "lihao@golia.jp" });
        let emails = serde_json::json!([
            { "email": "lihao@golia.jp", "primary": true, "verified": false },
            { "email": "real@users.noreply.github.com", "primary": false, "verified": true },
        ]);
        let id = identity_from_github(&p, &user, &emails).expect("identity");
        assert_eq!(
            id.email.as_deref(),
            Some("real@users.noreply.github.com"),
            "the unverified profile address must not become the identity"
        );
        assert_eq!(id.subject, "12345");
    }

    #[test]
    fn github_prefers_the_primary_verified_address() {
        let p = provider("github", IdentitySource::UserinfoThenEmails);
        let user = serde_json::json!({ "id": 7, "login": "x" });
        let emails = serde_json::json!([
            { "email": "other@x.com", "primary": false, "verified": true },
            { "email": "main@x.com", "primary": true, "verified": true },
        ]);
        assert_eq!(
            identity_from_github(&p, &user, &emails)
                .expect("identity")
                .email
                .as_deref(),
            Some("main@x.com")
        );
    }

    #[test]
    fn github_with_no_verified_address_yields_nothing() {
        let p = provider("github", IdentitySource::UserinfoThenEmails);
        let user = serde_json::json!({ "id": 7, "login": "x" });
        let emails =
            serde_json::json!([{ "email": "a@b.com", "primary": true, "verified": false }]);
        assert!(identity_from_github(&p, &user, &emails).is_err());
    }

    #[test]
    fn a_refusal_is_reported_as_one() {
        let p = provider("portal", IdentitySource::IdToken);
        let err = parse_token_response(&p, br#"{"error":"invalid_grant"}"#).expect_err("refused");
        assert!(matches!(err, OauthError::Refused { .. }));
    }

    #[test]
    fn a_token_response_carries_both_tokens() {
        let p = provider("portal", IdentitySource::IdToken);
        let t = parse_token_response(&p, br#"{"access_token":"at","id_token":"it"}"#).expect("ok");
        assert_eq!(t.access_token, "at");
        assert_eq!(t.id_token.as_deref(), Some("it"));
    }

    #[test]
    fn jwt_claims_decode_without_padding() {
        // {"iss":"https://portal.golia.jp","sub":"u1"}
        let payload = "eyJpc3MiOiJodHRwczovL3BvcnRhbC5nb2xpYS5qcCIsInN1YiI6InUxIn0";
        let claims = jwt_claims_unverified(&format!("h.{payload}.sig")).expect("claims");
        assert_eq!(claims["sub"], "u1");
    }

    /// The verifier goes in the exchange and never in the URL — that split is
    /// the whole of PKCE.
    #[test]
    fn the_verifier_is_in_the_exchange_and_not_the_url() {
        let p = provider("portal", IdentitySource::IdToken);
        let pkce = Pkce::from_verifier("secret-verifier");
        assert!(!authorize_url(&p, "s", &pkce).contains("secret-verifier"));
        assert!(
            token_request_body(&p, "code", &pkce, "cs").contains("code_verifier=secret-verifier")
        );
    }
}
