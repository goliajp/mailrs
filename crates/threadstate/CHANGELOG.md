# Changelog

All notable changes to `mailrs-threadstate` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0]

### Added

- Initial release.
- `Record`: one per-thread decision (`snoozed_until`, `category`,
  `importance_level`, `importance_score`, `requires_action`), stating
  only the fields it changes. `Record::new`, `Record::is_empty`.
- `ThreadState`: the log resolved per thread, the later record in file
  order winning. `parse`, `get`, `iter`, `len`, `is_empty`,
  `compacted`.
- File helpers for the NDJSON log beside a mailbox: `path`, `read`,
  `append`, `append_many`, `rewrite`, and `FILE_NAME`.
