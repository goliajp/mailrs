# Changelog

All notable changes to `mailrs-uidlist` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0]

### Added

- Initial release.
- `UidList`: the Dovecot `dovecot-uidlist` format, version 3. `new`,
  `parse`, `render`, `filename`, `uid_of`, `compacted`. Extension
  fields such as `W<size>` and `G<guid>` are read and ignored.
- File helpers for the list beside a mailbox: `path`, `read`, `append`,
  `append_many`, `rewrite`, and `FILE_NAME`. `read` derives the next
  UID as `max(header N, highest UID + 1)`, so `append` never has to
  rewrite the header.
