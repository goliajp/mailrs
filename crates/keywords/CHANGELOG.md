# Changelog

All notable changes to `mailrs-keywords` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-10-22

### Added

- Initial release. Reader and writer for the Dovecot `dovecot-keywords`
  format: the map from a Maildir++ keyword bit (a lowercase letter in the
  `:2,` suffix) to the keyword name it stands for.
- `Keywords` map with `new`, `parse`, `render`, `letter`, `name`,
  `names_of`, `is_empty` and `intern`. `intern` assigns the lowest free
  index to a new name and never renumbers an existing one; it returns
  `None` once all 26 letters are taken.
- `parse` skips malformed lines instead of failing, and keeps the first
  name when an index is repeated.
- File helpers `path`, `read` (an absent file reads as an empty map) and
  `write` (atomic replace), plus the constants `FILE_NAME`
  (`mailrs-keywords`) and `MAX_INDEX` (25).
