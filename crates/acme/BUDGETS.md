# mailrs-acme performance budgets

This crate intentionally **does not ship perf budgets** — all
hot paths are network-bound (ACME protocol over HTTPS, DNS for
challenge validation, file I/O for cert persistence).

## What this crate's actual cost looks like

| Operation | Typical wall-clock |
|---|---|
| `load_or_create_account` (cached) | ~10-50 ms (file I/O) |
| `load_or_create_account` (create) | ~1-5 s (ACME directory + nonce + account creation HTTPS) |
| `provision_cert` per domain | ~10-60 s (multiple ACME round trips, propagation wait) |
| `cert_days_remaining` (PEM parse) | < 1 ms (x509 parse) |
| Renewal task tick (idle) | < 1 ms (file mtime check) |

None of these are budget-able at the µs level — wall-clock is
dominated by:
- HTTPS round trips to the ACME directory
- DNS propagation delay for HTTP-01 challenge
- File I/O for cert persistence
- Polling interval between order state checks

## What CPU paths might warrant a budget

- `cert_days_remaining` — pure PEM + x509 parse. ~10-100 µs. If
  we ever called it in a tight inner loop, gate it; today it runs
  once every 12 hours from the renewal task.
- `build_server_config` — PEM parse + rustls config build. ~ms.
  Called on each renewal swap, never on the hot accept path.

## When to add budgets

If a future test harness needs reproducible ACME orchestration
timing (e.g. against a Pebble test server), the right shape is an
integration test, not a microbench.
