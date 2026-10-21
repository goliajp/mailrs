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

### Added

- `extract_pdf_pages(data)`: embedded text of each page, in page order.

## [1.0.1] - 2026-08-04

### Changed

- Source formatting, new benchmarks, a perf-regression test and a fuzz
  target. No API or behaviour change.

## [1.0.0] - 2026-05-24

### Added

- Initial release.
- `extraction_method(content_type)` and `ExtractionMethod`: choose PDF
  text extraction, OCR or nothing from a MIME type.
- `extract_pdf_text(data)`: embedded PDF text (confidence `1.0`).
- `ocr_image(data, langs)` and `tesseract_available()`: OCR through the
  `tesseract` CLI as a subprocess.
- `extract_content(...)`: dispatch by content type, falling back to OCR
  for scanned PDFs whose embedded text is too short.
- `ExtractionResult`: text, language hint, confidence, page count and
  JSON metadata.
- `MAX_EXTRACT_SIZE` (50 MiB) input limit.
