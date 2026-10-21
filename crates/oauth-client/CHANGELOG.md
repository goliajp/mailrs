# Changelog

All notable changes to `mailrs-oauth-client` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `TokenResponse::refresh_token` and `TokenResponse::expires_in`, read
  from the token endpoint's answer. **Breaking** for code that builds
  `TokenResponse` with a struct literal.
- `mail` module (re-exported at the crate root) for keeping a mailbox
  connected over OAuth: `mail_provider(key)`, `mailbox_scopes(key)`,
  `refresh_request_body(provider, refresh_token, client_secret)`,
  `needs_refresh(expires_at, now)` and `RENEW_WITHIN_SECS`.
- `net` feature (off by default) with `renew(...)`, `Renewed` and
  `RenewError`: exchange a refresh token for a new access token over
  HTTP using `reqwest`.

### Changed

- `LICENSE-APACHE` and `LICENSE-MIT` are now shipped with the crate.

## [0.1.0] - 2026-08-04

### Added

- Initial release. OAuth 2.0 / OpenID Connect relying-party primitives
  that make no network calls themselves.
- `Provider`: one provider's endpoints, client id, scopes and where its
  identity claims live (`IdentitySource`).
- `Pkce::generate` / `Pkce::from_verifier`: PKCE verifier and S256
  challenge.
- `authorize_url(provider, state, pkce)`, `token_request_body(...)` and
  `parse_token_response(provider, body)` returning `TokenResponse`.
- `jwt_claims_unverified(token)`, `identity_from_claims(...)` and
  `identity_from_github(...)`, producing an `ExternalIdentity`.
- `OauthError` for malformed provider answers.
