# Changelog

All notable changes to `mailrs-ical` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `reply` module for RFC 5546 `REPLY` messages: `build` and
  `serialize_reply` produce a reply carrying only the replying attendee,
  `reply_message` wraps it in a complete RFC 5322 message, plus
  `organizer_of`, `role_of`, `partstat_from_wire` and `subject_for`.
- `instants` module: `instant_of` / `instants_of` resolve start and end
  times to UTC using the invite's `VTIMEZONE` blocks, and `join_link`
  finds an online meeting URL on a known conferencing host.
- `mime_part::extract_invite_part` (behind the new `mime` feature, which
  pulls in `mailrs-mime`): finds the `text/calendar` part of an inbound
  message, inline or as an attachment, and returns it as an
  `ExtractedInvite` with the `method=` parameter.
- `Deserialize` is implemented for the public model types
  (`ParsedInvite`, `Attendee`, `Person`, `CalDateTime`, `VTimezone`,
  `RawComponent`, `RawProperty`, `Method`, `PartStat`, `Role`,
  `EventStatus`).

## [2.1.0] - 2026-08-04

### Added

- `parse::split_vevents(text)` splits a calendar feed into one complete
  `VCALENDAR` document per `VEVENT`. An unterminated trailing event is
  dropped.

### Changed

- Serialization writes into the output buffer with `write!` instead of
  allocating intermediate strings. Output is unchanged.

## [2.0.0] - 2026-05-29

### Changed (breaking)

- Short name fields are `compact_str::CompactString` instead of
  `String`: `RawComponent::name`, `RawProperty::name`,
  `RawProperty::params` (both halves), `VTimezone::tzid`,
  `CalDateTime::Zoned { tz_name }` and `Person::cn` /
  `Attendee::cn`. Values and e-mail addresses stay `String`.
- New dependency: `compact_str`.

## [1.0.3] - 2026-05-23

### Added

- `benches/compare_icalendar.rs` — head-to-head bench vs the `icalendar` 0.17
  crate (the most popular Rust iCalendar parser).

### Performance

Measured (criterion, M-series Mac, release, `--quick`):

| Input | mailrs-ical | icalendar 0.17 |
|---|---:|---:|
| simple VEVENT | **1.44 µs** | 5.33 µs (3.7×) |
| VEVENT + RRULE | **1.63 µs** | 5.96 µs (3.7×) |
| VTIMEZONE + VEVENT | **2.67 µs** | 9.21 µs (3.4×) |

Clean sweep on the parse path. icalendar's builder / serializer APIs are
broader than ours; we don't bench those.

No lib code change.

## [1.0.2] - 2026-05-22

### Added
- `#![deny(missing_docs)]` gate; all public items now carry rustdoc.
- Perf regression gates under `tests/perf_gate.rs` with documented budgets in `BUDGETS.md`.

### Changed
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.
- Refreshed dev dependencies (criterion 0.7 → 0.8).

## [1.0.1] - 2026-05-20

### Added
- Deeper test coverage and criterion benches for parser and serializer hot paths.

### Removed
- Crate-level `#[allow(dead_code)]` blanket — replaced with targeted, justified allows.

## [1.0.0] - 2026-05-20

### Added
- Initial release. Hand-rolled RFC 5545 iCalendar parser, serializer, and iTIP semantics with `VTIMEZONE` and `RRULE` support. Zero I/O.
