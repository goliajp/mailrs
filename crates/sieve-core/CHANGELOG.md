# Changelog

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
