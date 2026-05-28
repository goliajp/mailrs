# Changelog

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
