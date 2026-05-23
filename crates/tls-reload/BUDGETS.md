# mailrs-tls-reload performance budgets

This crate intentionally **does not ship perf budgets** — the
operations it owns are sub-microsecond atomic primitives wrapping
`arc-swap`, which has its own perf coverage upstream.

## What this crate's hot paths actually are

- **`TlsState::acceptor()`** — one `ArcSwap::load_full` (~1-10 ns).
  Called per accepted TLS connection.
- **`TlsState::swap()`** — one `ArcSwap::store` (~1-10 ns). Called
  rarely (on cert renewal).
- **`TlsState::current()`** — one `ArcSwap::load_full` clone. Same
  cost as `acceptor()`.
- **`load_tls_config()`** — file I/O + PEM parse + rustls
  ServerConfig::builder. Wall-clock dominated by syscalls. Not a
  per-connection path.

## Why no perf_gate.rs

A regression test on `acceptor()` at the nanosecond level would
just measure compiler / scheduler noise, not crate-level behavior.
The arc-swap crate provides upstream guarantees we rely on.

For renewal load-test scenarios, the bench harness lives in
`mailrs-server`'s perf surface where it's measured end-to-end
(TLS handshake + cert swap + post-swap handshake).

## When to add budgets

If a future change replaces `arc-swap` with a different sync
primitive (e.g. parking_lot RwLock), add an integration perf test
comparing the two — the trait shape is what matters, not the
specific load latency.
