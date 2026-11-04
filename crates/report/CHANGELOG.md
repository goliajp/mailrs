# Changelog

All notable changes to `mailrs-report` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-11-04

### Added

- Initial release.
- `fingerprint::of_message` / `of_text`: a 64-bit SimHash of the
  normalised body; `segments` and `distance` for indexing and comparing.
- `template::same`: one template when the hashes are at most 3 bits
  apart and the messages share a link domain, an attachment or the
  sender domain.
- `decide`: whether a report holds only the reporter's copy, takes the
  other copies back, or waits for an administrator.
