# Changelog

All notable changes to `mailrs-attachment-extract` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- PDF text extraction no longer uses `pdf-extract`. Text comes from an
  own content-stream interpreter on `lopdf` 0.45: ToUnicode CMaps,
  predefined CJK CMaps (`UniJIS-UCS2-H`, `90ms-RKSJ-H`, ...),
  Adobe-Japan1 CIDs, base encodings with `/Differences` through the
  Adobe Glyph List, and embedded TrueType `cmap` / `post` tables.
  Pages are separated by `\u{000C}`, so `page_count` is now accurate.
  Malformed files that used to panic now return what text they have.

### Added (v3 stones polish, 2026-05-25)

- BUDGETS.md describing the perf-gate methodology
- `tests/perf_gate.rs` with at least one regression gate
- `benches/attachment-extract.rs` criterion harness
- README "Stone audit" footer with cov / size / bench / fuzz status
