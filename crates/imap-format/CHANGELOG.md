# Changelog

All notable changes to `mailrs-imap-format` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.0] - 2026-10-22

### Added

- `build_envelope(data)` (module `envelope`): the FETCH `ENVELOPE`
  structure (RFC 9051 §7.5.2) built from a message's header block.
  `Sender` and `Reply-To` default to `From`; other missing fields are
  `NIL`. Field values are sent as they appear on the wire.
- `extract_header_fields_except(data, fields)` for
  `BODY[HEADER.FIELDS.NOT (...)]`.

### Changed

- New dependency: `mailrs-rfc5322` 1.2 (address list splitting for the
  envelope).

## [1.0.1] - 2026-08-04

### Changed

- MIME helpers moved into the `mime` module and `build_bodystructure`
  into the `bodystructure` module; both are re-exported at the crate
  root, so existing paths keep working.
- Header and body scans use `memchr` (1.6 to 7.7 times faster), and
  `format_imap_flags` / `parse_imap_flags` are about twice as fast.
  Output is unchanged.
- New dependency: `memchr`.

## [1.0.0] - 2026-05-23

### Added

- Initial release. Pure IMAP wire-format helpers, no I/O:
  - flags: `FLAG_SEEN`, `FLAG_ANSWERED`, `FLAG_FLAGGED`,
    `FLAG_DELETED`, `FLAG_DRAFT`, `FLAG_RECENT`, `format_imap_flags`,
    `parse_imap_flags`;
  - strings and dates: `format_internal_date`, `escape_imap_string`,
    `escape_imap_str`, `quote_or_nil`, `format_imap_address`,
    `format_addr_list`;
  - FETCH sections: `parse_header_fields_request`,
    `parse_generic_body_sections`, `extract_header_fields`,
    `extract_header_section`, `extract_body_section`,
    `extract_mime_part`;
  - MIME: `MimeInfo`, `split_mime_parts`, `find_line_offset`,
    `trim_part_trailing_newline`, `build_bodystructure`.
