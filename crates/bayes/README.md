# mailrs-bayes

Naive-Bayes spam classifier core — the statistical engine behind
mailrs's self-hosted anti-spam. No third-party spam service (no
rspamd, SpamAssassin, or cloud API): 100% owned, offline, explainable
per token.

- **`tokenize(&[u8]) -> Vec<String>`** — RFC 5322 bytes into a
  deduplicated feature-token set. Body words + namespaced feature
  tokens (`sub:` subject, `from:` sender domain, `url:` link domain,
  `hdr:ct:` / `hdr:charset:`). CJK runs are bigram-split.
- **`classify(tokens, lookup, corpus) -> Option<f64>`** —
  Graham-Robinson per-token probabilities with Fisher chi-square
  combining over the 15 most discriminatory tokens. `None` = the
  corpus hasn't cleared the cold-start gate (200 total / 50 spam /
  50 ham), so an untrained deployment sees zero effect.

Pure functions, no I/O: the caller injects token counts (from a KV
store, a SQL table, whatever) and persists training deltas. Training
signal in mailrs comes from the user's mark-junk / mark-not-junk
actions — the data-sovereignty loop.

Design: `.claude/rfcs/20260713-bayes-antispam-engine.md`.

## License

Apache-2.0 OR MIT.
