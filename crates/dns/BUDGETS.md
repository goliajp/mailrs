# mailrs-dns performance budgets

This crate intentionally **does not ship perf budgets** — all
operations are network-bound (DNS over UDP/TCP), with wall-clock
times in the range of 1-100 ms depending on resolver path, cache
state, and remote DNS server load.

## What this crate's hot paths actually are

- `lookup_txt` / `_a` / `_aaaa` / `_mx` / `_ptr` — each is a single
  DNS query over UDP first, falling back to TCP for large responses.
- Caller-supplied resolver (hickory-resolver via the `hickory`
  feature) owns connection pooling, retry, and cache.

## Why no perf_gate.rs

There's no useful CPU budget to gate. The only CPU work is:
- Iterating answer records (microseconds)
- String / IpAddr extraction (sub-microsecond)

A regression test on that would measure noise, not bug-detection
signal.

## When to add budgets

If a future version adds an in-process cache (currently deferred to
1.1), then cache hit / miss timing would warrant a budget table.
That's the natural moment to revisit this file.
