# mailrs-server performance budgets

Server-level (cement) regression budgets. Two layers:

- **Pure-logic hot-path gates** — `tests/perf_gate.rs`, the stone
  composition an inbound delivery follows (smtp-proto session + parse →
  maildir deliver, rfc5322 lookup, dmarc evaluate). Named `*_under_budget`,
  µs/ms budgets at ~5-10× headroom. Skipped on CI (`--skip _under_budget`,
  noisy runners) — run locally / before ship.
  `cargo test -p mailrs-server --test perf_gate`

- **End-to-end receiving latency** — `tests/e2e_receiving.rs`, the real
  SMTP-over-TCP → maildir → index → event path against a pgvector
  testcontainer. Runs on CI (PG axis); budgets here are deliberately loose
  so they gate behavior (did async processing complete?) not micro-perf.

## End-to-end budgets

| Span | Budget | Actual (M-series, in-process) | Rationale |
|---|---|---|---|
| delivery → `NewMessage` event | < 2 s | tens of ms | S2.3. Post-delivery is async (S1.4 mpsc consumer): the DATA handler returns 250 without waiting for the index, and the consumer emits `NewMessage` after `index_message`. The 2 s ceiling is a stall detector — if the consumer wedges or the channel backpressure path degrades, this trips. Measured from delivery start (connect), a conservative upper bound on the maildir-write → emit span. |

## How to add a new e2e budget

1. Capture an `Instant` at the start of the span and the elapsed when the
   terminal event / DB state is observed.
2. Pick a loose ceiling (≥ 20× the observed value) — these run on noisy CI
   runners, so they must catch order-of-magnitude regressions only.
3. Document the span, budget, and reasoning here.
