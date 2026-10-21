# mailrs-oauth-client

[![Crates.io](https://img.shields.io/crates/v/mailrs-oauth-client?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-oauth-client)
[![docs.rs](https://img.shields.io/docsrs/mailrs-oauth-client?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-oauth-client)
[![License](https://img.shields.io/crates/l/mailrs-oauth-client?style=flat-square)](#license)

OAuth 2.0 / OpenID Connect relying-party primitives: authorization URLs
with PKCE, token exchange, and reading a person's identity from
providers that each describe it differently.

The crate makes no network calls by default. The token exchange and,
for providers without an `id_token`, the userinfo fetch are done by the
caller, which passes the response bodies in as bytes. That keeps URL
building, PKCE derivation and claim reading testable without a network.

The question this crate answers is "who is this", not "whose mailbox is
this". An identity provider states which account it authenticated; an
email claim is that provider's opinion about a string. What to do with
an `ExternalIdentity` (link it, require a password first, refuse it) is
left to the caller.

## API

- `Provider`: one configured provider (endpoints, client id, redirect
  URI, scopes, pinned issuer) and `IdentitySource`, which says whether
  the identity is in the `id_token` or behind a userinfo call.
- `Pkce::generate()` / `Pkce::from_verifier(..)`: PKCE verifier and
  S256 challenge.
- `authorize_url(provider, state, pkce)`: where to send the browser.
- `token_request_body(..)` and `parse_token_response(provider, body)`:
  the code exchange; `TokenResponse` carries the access token, optional
  `id_token`, refresh token and lifetime.
- `jwt_claims_unverified(token)`, `identity_from_claims(provider, claims)`
  (refuses an issuer that does not match the pinned one) and
  `identity_from_github(provider, user, emails)` (uses only verified
  addresses from `GET /user/emails`).
- Mailbox access over OAuth: `mail_provider(key)`,
  `mailbox_scopes(key)`, `refresh_request_body(..)`,
  `needs_refresh(expires_at, now)` and `RENEW_WITHIN_SECS`.

## Features

- `net` (off by default): `renew(provider, refresh, client_secret,
  account, now)` performs the refresh-token exchange over HTTP with
  `reqwest`, returning `Renewed` or `RenewError`. `RenewError::Refused`
  is the only failure that means the user must sign in again.

## License

Apache-2.0 OR MIT.
