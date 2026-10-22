# Changelog

All notable changes to `mailrs-textguard` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-10-22

### Added

- Initial release.
- `deception_in(&str) -> Deception`: checks decoded identifying text
  (a From display name, a subject) for characters placed there to
  deceive. The zero-width joiner, zero-width non-joiner, byte-order mark
  and soft hyphen in their legitimate roles are not flagged.
- `deception_in_any(iter)`: the same over several fields, folded into
  one result.
- `Deception` with `bidi_override`, `unjustified_zero_width` and
  `zero_width_inside_a_word`, and `Deception::any()`.
