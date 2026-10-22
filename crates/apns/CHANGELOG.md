# Changelog

All notable changes to `mailrs-apns` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.1] - 2026-10-22

### Changed

- License changed from `MIT` to `Apache-2.0 OR MIT`; both license texts
  are now shipped with the crate.
- Documentation: rustdoc builds without broken intra-doc links. No API or
  behaviour change.

## [1.0.0] - 2026-08-07

### Added

- Initial release.
- `ApnsClient::new(key_pem, key_id, team_id, topic, endpoint)` — token-based
  client built from a `.p8` PKCS#8 EC key. ES256 provider JWTs are cached and
  refreshed before Apple's one-hour limit.
- `ApnsClient::send_alert(device_token, title, body)` — HTTP/2 alert push
  via reqwest.
- `Outcome` — `Sent`, `Unregistered` (`410` or `BadDeviceToken`, so callers
  can delete dead tokens) or `Failed { status, reason }`.
- `ApnsError` — key parsing, signing and transport errors.
- `PRODUCTION_ENDPOINT` and `SANDBOX_ENDPOINT` constants; the endpoint is a
  constructor argument so the sandbox gateway or a local stub can be used.
