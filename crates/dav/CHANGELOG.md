# Changelog

All notable changes to `mailrs-dav` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.0] - 2026-10-22

### Changed

- **Breaking:** `DavError` has a new variant, `UidConflict(String)`: 403
  with the CalDAV `no-uid-conflict` precondition (RFC 4791 §5.3.2.1),
  carrying the href of the resource that already holds the `UID`.
  `to_dav_response` renders the precondition body. Migration: add an arm
  for it to exhaustive `match`es on `DavError`.
- **Breaking:** the address-book home is `/dav/addressbooks/{user}/` (was
  `/dav/contacts/{user}/`), in the principal's
  `addressbook-home-set` and every href the CardDAV handlers emit.
  Migration: route `/dav/addressbooks/` to the CardDAV handlers (or use
  `dispatch::handle`); clients that discover the home through the
  principal follow it without change.
- **Breaking:** `CS:getctag` is a digest of the member etags, so it
  changes whenever a member does; it carried the collection description
  before. The description moved to `C:calendar-description` /
  `CR:addressbook-description`. Single collections report it at
  `Depth: 0`. Migration: read the description from those properties.
- `Event::uid` is the resource name from the URL. It is usually the
  iCalendar `UID`, but a client may PUT an event under a name of its own;
  a store that keys events by `UID` maps one to the other and refuses a
  PUT whose `UID` another resource has with `DavError::UidConflict`.
- Handlers convert store errors with `store::into_dav_error`, so a store
  that returns a boxed `DavError` keeps its status (403, 400, ...)
  instead of it becoming a 500.
- Collection names and UIDs are percent-encoded in hrefs and XML-escaped
  in display names; the collection-level handlers take the decoded name.
  Multiget hrefs are decoded before matching.
- `OPTIONS` no longer advertises `MKCALENDAR`, which nothing implements,
  and lists `HEAD`.
- `DavError` implements `PartialEq` / `Eq`.

### Added

- `dispatch::handle` routes a whole `/dav/` tree by path and method, and
  answers 403 when the path's user segment is not the authenticated user.
- `caldav::calendar_home_report` / `carddav::addressbook_home_report`: a
  REPORT sent to the home runs over every collection.
- `store::into_dav_error`: a store may return a boxed `DavError` to choose
  the status (403, 400, ...); any other error is still a 500.
- `caldav::event_href`, `parse::encode_segment`, `parse::percent_decode`,
  `parse::multiget_uids` and `xml::ctag_of`.

## [1.1.2] - 2026-08-04

### Changed

- Multistatus responses are built with `write!` instead of
  `push_str(&format!(..))`, avoiding an allocation per response entry.
  Output is unchanged.
- The `fixtures` module is split into submodules and re-exports the same
  items. No API change.

## [1.1.1] - 2026-05-23

### Added

- 39 new inline tests across `carddav.rs` (+22) and `caldav.rs` (+17)
  covering precondition handling and report semantics:
  - Contact / event PUT: `201` on create vs `204` on update,
    `If-Match` correct/wrong etag, `If-None-Match: *` for new vs
    existing resources.
  - Address book / calendar PROPFIND: `Depth: 0` returns only the
    collection, `Depth: 1` lists children, default-collection
    creation is idempotent across repeated home-PROPFINDs.
  - REPORT multiget with empty UID list, with missing UIDs, and
    `calendar-query` / `addressbook-query` no-filter forms.
  - DELETE on missing event/contact returns `NotFound`.
  - `urlencode` byte-level assertions for alphanumeric pass-through,
    space encoding, special chars, and per-byte UTF-8 (Japanese).
- Lib test count: 44 → 83.

No lib code change.

## [1.1.0] - 2026-05-21

### Added
- Public in-memory `CalendarStore` / `AddressBookStore` fixtures, promoted from `dev-dependencies` so downstream crates can reuse them in their own tests and examples.
- Perf regression gates under `tests/perf_gate.rs` covering handler dispatch and composition, with documented budgets in `BUDGETS.md`.
- Dropped the unenforceable `rust-version = "1.85"` declaration from `Cargo.toml`.

## [1.0.3] - 2026-05-21

### Added
- Criterion benchmarks for handler dispatch and request composition under `benches/`.

## [1.0.2] - 2026-05-21

### Added
- Protocol-level integration coverage: end-to-end CalDAV and CardDAV scenarios against the in-memory store.

## [1.0.1] - 2026-05-21

### Added
- `#![deny(missing_docs)]` gate; all 22 public items now carry rustdoc.
- Expanded README with quick-start example and trait overview.

## [1.0.0] - 2026-05-20

### Added
- Initial release. Framework-agnostic CalDAV (RFC 4791) and CardDAV (RFC 6352) server-side handlers, with the data layer pluggable via the `CalendarStore` / `AddressBookStore` traits.
