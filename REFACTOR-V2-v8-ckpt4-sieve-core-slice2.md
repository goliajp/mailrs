# v8 ckpt 4 (second slice) — sieve-core 0.1.x

Second incremental slice of v8 ckpt 4 work. Triples the differential
corpus (10 → 30 scripts, all green) and fixes the `stop` short-circuit
hole noted in slice 1.

## What changed

### `src/eval.rs` — `stop` proper short-circuit

`stop` now sets a `stopped: bool` flag checked at the top of every
`eval_block` iteration, so the outer block unwinds without running
any commands after `stop`. Previously `stop` set `explicit_action`
but the block kept iterating — slice 1's only known divergence
from `sieve-rs`.

### `tests/diff_sieve_rs.rs` — corpus 10 → 30

20 new differential rows covering the gaps slice 1 left:

| group | rows added |
|---|---:|
| header :matches glob (`*…*`, `?`) | 2 |
| not invert | 2 |
| allof / anyof (true + false + mixed) | 4 |
| address :localpart / :domain | 4 |
| if/elsif/else chain branches | 2 |
| stop short-circuit | 1 |
| redirect / reject / case-insensitive :is | 3 |
| exists multi (string-list + missing) | 2 |

All 30 corpus rows: both engines produced identical `NormalizedAction`
sequences. Test runtime ~ a few ms.

## Carve-outs → slice 3

- **`require` enforcement (strict)**: sieve-rs hard-errors when a
  script uses `fileinto` / `reject` / extension without the
  matching `require`. `mailrs-sieve-core` 0.1.x is permissive
  (treats `require` as advisory). Adding strict enforcement would
  break the 50 inline unit tests that elide `require` for
  brevity — slice 3 introduces both the enforcement and the test
  updates in one go.
- **`:comparator "i;octet"`** — RFC 4790 case-sensitive comparator
  variant. Current corpus doesn't exercise the gap.
- **Extension expansion**: vacation / imap4flags / mime / etc.
  (ckpt 5 work, not slice 3).

Neither carve-out causes any of the 30 current corpus rows to
disagree, so the slice ships as-is.

## Trigger status

Per v8 RFC ckpt 4 → 5 trigger:

> RFC 5228 base 实现完 · 200 个 differential script 100% 一致 vs sieve-rs · workspace clippy + test 全绿

| | required | now |
|---|---|---|
| RFC 5228 base implemented | yes | ✓ (slice 1 + 2) |
| 200 differential scripts agree | 200 | 30 (15 %) |
| workspace clippy + test green | yes | ✓ |

The 200-script target is structural — each subsequent slice adds
30-50 rows until parity. No prod risk between slices (the wrapper
still routes to `sieve-rs`).

## Tests

| File | Count | Notes |
|---|---:|---|
| inline (lex/ast/parse/eval) | 50 | unchanged from slice 1 |
| `tests/diff_sieve_rs.rs` | 1 × 30 corpus rows | slice 2 expansion |
| **Total** | **80+ effective assertions** | |
