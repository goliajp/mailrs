# Changelog

## 0.1.4 (unreleased)

### Added

- Differential corpus grown 142 → **202 scripts** (`slice4_e` +
  `slice4_f` + `slice4_g`). **ckpt 4 → 5 trigger gate 100%
  satisfied** (200/200 spec target met). New categories: advanced
  `:matches` glob (consecutive stars, anchored), UTF-8 / non-ASCII
  in strings (Japanese subject + localpart), 4-action chains,
  multiple top-level if statements, deep allof/anyof nesting (4
  branches), List-Id / List-Unsubscribe / Priority header filters,
  edge sizes (1K-boundary, anyof with size), real-world filter
  shapes (reply-thread, calendar invite, X-Spam-Status), address
  test variants (`:is` / `:contains` / `:matches`), exists corner
  cases (single-form, partial-present, all-missing+not), require
  with extension placeholders (imap4flags / subaddress as no-ops),
  case-sensitivity coverage, comments in deep positions, sieve
  syntax edges (newlines inside test args, extra whitespace).

### Notes

- Two corpus rows were intentionally omitted because they surface
  a spec-interpretation difference (not a bug):
  - `keep; fileinto X;` — sieve-rs collapses `keep` when a
    subsequent `fileinto` fires; sieve-core emits both literally.
  - `discard; fileinto X;` — sieve-rs collapses similarly.
  - Both behaviours are RFC 5228 compliant. sieve-core (zero-I/O
    stone) leaves dedup to the caller (delivery layer).

## 0.1.3 (unreleased)

### Added

- Differential corpus grown 100 → 142 scripts (`slice4_c` + `slice4_d`),
  pushing ckpt 4 → 5 trigger gate progress to 142/200 (71%). New
  categories: multi-line `text:` strings in scripts, number edges
  (0 / huge / exact-kilobyte), whitespace tolerance, header-value
  edges (empty `Subject`, `:matches "*"`), address shape edges
  (dotted localpart, subdomain), message-shape edges (no body),
  comments in unusual positions, deep nesting variants, action
  sequence semantics including `stop` inside `else`, `require`
  edges (no action / repeated calls), real-world filter shapes
  (newsletter / VIP priority / auto-archive).

### Changed

- File-size hard limit closed: every file ≤ 500, every function ≤ 200.
  - `src/lex.rs` (523) → `src/lex/mod.rs` (445) + `src/lex/string.rs` (153).
  - `src/eval.rs` (501) → `src/eval/mod.rs` (365) + `src/eval/context.rs`
    (35) + `src/eval/test_engine.rs` (131).
  - `tokenize` function 235 lines → 164 lines after string-scanning
    extraction.
- Slice 1/2 inherited file-size debt fully closed.

## 0.1.2 (unreleased)

### Added

- Differential corpus grown 65 → 100 scripts (`corpus_slice4_a` +
  `corpus_slice4_b`), pushing ckpt 4 → 5 trigger gate progress to
  100/200 (50%). New categories: comments (`#` line + `/* block */`),
  escape sequences in quoted strings, numbers with K/M/G suffix,
  long elsif chains (5+ levels), deeply nested `if` (4 levels),
  multi-action sequences, `require` with multi-extension lists,
  empty/minimal blocks, nested `allof(anyof, allof)`, `not` around
  `allof` / `anyof`, multi-recipient `address` tests against
  `To` + `Cc`, case-insensitive header lookup, address-part edges
  (`:all` / `:localpart` / `:domain` exact matches).

### Fixed

- `stop` (RFC 5228 §4.5) no longer cancels the implicit keep. Slice 2
  set `explicit_action = true` by mistake; slice 4 corpus row
  `stop_at_top_level_before_keep` surfaced the divergence vs
  `sieve-rs`. Fix is one removed line in `eval.rs`'s `stop` arm.

### Changed

- Differential test framework: `sieve-rs` is now configured with
  `max_redirects = usize::MAX`. Default 1 is a sieve-rs anti-mail-loop
  policy, not an RFC 5228 requirement — `sieve-core` (zero-I/O stone)
  leaves the decision to the caller. The lift makes the differential
  comparison fair across multi-redirect scripts.
- Corpus moved out of `tests/diff_sieve_rs.rs` into
  `tests/common/corpus/` (per-slice sub-modules). The diff test
  driver is now ~30 lines.

## 0.1.1 (unreleased)

### Added

- RFC 5230 `vacation` action — `Action::Vacation(VacationAction)`
  surfaces parsed `reason`, `:days` / `:seconds` window, `:subject`,
  `:from`, `:addresses`, `:mime` flag, and `:handle` to the caller.
  Stateful parts (dedup, recipient detection, reply-message build)
  remain the caller's job, keeping the engine zero-I/O.
- 11 new inline unit tests in `vacation.rs` covering RFC 5230 §3-4
  (parsing of every tag, implicit-keep preservation, dual-tag
  conflicts).
- Differential corpus grown from 32 → 65 scripts (`corpus_slice3`),
  ramping ckpt 4 → 5 trigger gate progress to 65/200 (32.5%).

### Changed

- Extracted `address` helpers (RFC 5228 §5.1) into
  `src/address.rs` to keep `eval.rs` under the project file-size
  limit after adding vacation dispatch.
- Extracted `match_string` / `glob_match` (RFC 5228 §2.7) into
  `src/match_str.rs` for the same reason — `eval.rs` is now 503
  lines (down from 576).
- Split the differential test framework into `tests/common/mod.rs`
  and corpus into `corpus_slice12()` + `corpus_slice3()` so every
  function stays ≤ 200 lines.

### Notes

- `vacation` is intentionally **excluded** from the cross-engine
  differential corpus: `sieve-rs` internalises message-building
  (emits `CreatedMessage` + `SendMessage` events), while
  `sieve-core` surfaces a single abstract `Action::Vacation`. The
  abstractions don't line up, so RFC 5230 spec coverage lives in
  `vacation.rs`'s inline unit tests instead.

## 0.1.0 (unreleased)

- Initial slice. RFC 5228 §2 tokenizer, §3-4 AST + parser, §4
  minimal evaluator.
- Actions: `keep`, `discard`, `fileinto`, `redirect`, `reject`.
- Tests: `header`, `address`, `size`, `exists`, `true`, `false`,
  `not`, `allof`, `anyof`.
- Match-types: `:is`, `:contains`, `:matches`.
- Address-parts: `:all`, `:localpart`, `:domain`.
- Differential-tested against `sieve-rs` on a 5-10 script
  smoke corpus.
