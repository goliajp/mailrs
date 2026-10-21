# Changelog

All notable changes to `mailrs-smtp-codec` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.2] - 2026-08-04

### Changed

- `has_smuggle_sequence`, `normalize_line_endings` and the DATA-mode
  terminator search are now `memchr`-based. On a 100 KB body the
  scanners are 2–31× faster; DATA-mode decoding is 2–3× faster in all
  three smuggle-protection modes. Output is unchanged.
- New dependency: `memchr` 2.

## [1.0.1] - 2026-05-24

### Changed

- More tests covering strict mode with a clean payload, `Off` mode,
  incomplete DATA input and the encoder. No change to the library.

## [1.0.0] - 2026-05-23

### Added

- Initial release.
- `SmtpCodec`: a tokio `Decoder`/`Encoder` for SMTP. Command mode
  yields CRLF-terminated lines capped at 1024 octets; after
  `enter_data_mode()` it yields the message body up to the
  `CRLF.CRLF` terminator.
- `SmtpCodec::with_smuggle_protection` and `with_max_message_size`
  builder methods.
- `SmtpInput` (`Command`, `Data`, `DataRejected`) for decoded frames.
- `SmuggleProtection` (`Strict`, `Permissive`, `Off`) policies against
  bare-LF SMTP smuggling.
- `has_smuggle_sequence` and `normalize_line_endings` as standalone
  functions.
