# Changelog

All notable changes to `mailrs-bayes` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- The Apache-2.0 and MIT license texts are shipped with the crate.

## [0.1.0] - 2026-08-04

### Added

- Initial release.
- `tokenize(raw)`: RFC 5322 bytes to a deduplicated set of feature
  tokens (body words plus namespaced subject, sender-domain, link-domain
  and content-type tokens; CJK runs split into bigrams).
- `classify(tokens, lookup, corpus)`: Graham-Robinson per-token
  probabilities with Fisher chi-square combining. Returns `None` until
  the corpus passes the cold-start minimum (200 messages, 50 spam,
  50 ham).
- `classify_multiclass(...)` with `MultiCorpus`: the same approach over
  more than two categories.
- `TokenCounts` and `Corpus`: the counts the caller supplies. The crate
  does no I/O; storage and training are the caller's.
