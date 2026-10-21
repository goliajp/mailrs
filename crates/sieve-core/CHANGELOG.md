# Changelog

All notable changes to `mailrs-sieve-core` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-08-04

### Changed

- String matching (`:is` / `:contains` / `:matches`) no longer allocates,
  and the lexer skips comments with `memchr` (new dependency). Results
  are unchanged; the public API is the same as 0.2.0.

## [0.2.0] - 2026-06-02

First release on crates.io. Versions 0.1.x were not published.

### Added

- `tokenize`, `parse_script` and `validate` for RFC 5228 scripts.
  `validate` rejects a script that uses an extension command, test or tag
  without the matching `require`.
- `eval_script(script, message)` and
  `eval_script_with_envelope(script, message, &Envelope)`, returning the
  list of `Action`s. The engine does no I/O: delivery, redirects and
  vacation replies are the caller's job.
- Actions: `keep`, `discard`, `fileinto`, `redirect`, `reject`, and
  RFC 5230 `vacation` (`Action::Vacation(VacationAction)` with the parsed
  reason, `:days` / `:seconds`, `:subject`, `:from`, `:addresses`,
  `:mime` and `:handle`; de-duplication and reply building stay with the
  caller).
- Tests: `header`, `address`, `envelope` (RFC 5228 §5.4, over the
  `from` / `to` / `auth` parts of `Envelope`), `size`, `exists`, `true`,
  `false`, `not`, `allof`, `anyof`.
- Match types `:is`, `:contains`, `:matches`; address parts `:all`,
  `:localpart`, `:domain`, and `:user` / `:detail` (subaddress).
- RFC 5232 `imap4flags`: `setflag`, `addflag`, `removeflag`, the
  `hasflag` test, and a `:flags` tag on `keep` and `fileinto`.
  `Action::Keep { flags }` and `Action::FileInto { mailbox, flags }`
  carry the flags; callers that ignore flags can ignore the field.

### Notes

- `stop` does not cancel the implicit keep (RFC 5228 §4.5).
- Actions are returned as written: `keep; fileinto "X";` yields both.
  Collapsing duplicates is left to the delivery layer.
- There is no redirect limit; loop protection is the caller's policy.
