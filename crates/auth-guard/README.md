# mailrs-auth-guard

[![Crates.io](https://img.shields.io/crates/v/mailrs-auth-guard?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-auth-guard)
[![docs.rs](https://img.shields.io/docsrs/mailrs-auth-guard?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-auth-guard)
[![License](https://img.shields.io/crates/l/mailrs-auth-guard?style=flat-square)](#license)

Per-IP + per-(IP, username) failed-auth counter with **exponential-backoff
lockout**. Sharded `DashMap` storage. **Allocation-free on the success
path** (the only path that runs on every legitimate login).

Generic — any service that accepts authenticated connections (SMTP
submission, IMAP, JMAP, SSH-style PAM, HTTP form login, …) can use this
to slow brute-force attackers without affecting honest users.

## Quickstart

```rust
use mailrs_auth_guard::{AuthGuard, AuthGuardConfig, AuthCheck};
use std::net::IpAddr;
use std::time::{SystemTime, UNIX_EPOCH};

let guard = AuthGuard::new(AuthGuardConfig::default());
let ip: IpAddr = "192.0.2.1".parse().unwrap();

// The caller supplies the clock as unix seconds — the guard stores no
// `Instant`, so the same `now` drives the window and lockout math.
let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

// Before checking the password, ask: are we already in lockout?
match guard.check(ip, "alice", now) {
    AuthCheck::Allowed => {
        // proceed to verify password
        let password_ok = false; // your real check
        if password_ok {
            guard.record_success(ip, "alice");
        } else {
            guard.record_failure(ip, "alice", now);
        }
    }
    AuthCheck::LockedOut { remaining_secs } => {
        // reject immediately; don't touch the password backend
        println!("locked out for {remaining_secs} more seconds");
    }
}
```

## Design

Two counters run in parallel, each with its own sliding window and
own lockout state:

- **Per-(IP, username)** — defeats "guess Alice's password 1000 times"
  attacks targeting a specific account
- **Per-IP** — defeats "spray a million usernames from one IP" attacks
  that wouldn't trip any single account's counter

The IP counter triggers regardless of which username was attempted,
so a username-sprayer eventually hits the IP-level lockout even if
no single username's counter ever fills.

**IPv6 normalized to /64 prefix** — a single attacker controlling
a /64 (a typical residential delegation) can't trivially evade by
hopping addresses within their own block.

**Exponential backoff** — repeat offenders see the lockout duration
double each time. Configurable multiplier + ceiling (default: cap at
24 hours).

**No allocation on the success path.** The check itself is two
DashMap reads + two `if let` branches. No string allocation, no
clone of the username. Allocations happen only when recording an
actual failure.

## When to call `cleanup_stale`

The maps grow proportionally with the number of distinct IPs that
have ever attempted auth. Under sustained attack volume (millions of
distinct source IPs) this can pile up. Run `cleanup_stale(now)` from
a background tokio task every few minutes to drop expired records.
Active records (in-window failures or unexpired lockouts) are
preserved.

## What this crate is not

- **Not a password verifier.** You bring your own password backend
  (argon2, bcrypt, LDAP, OAuth, …).
- **Not a rate limiter for general HTTP requests.** Use
  [`mailrs-rate-limit`](https://crates.io/crates/mailrs-rate-limit)
  for that — it's a token-bucket store keyed by arbitrary `&str`.
- **Not a distributed store.** State lives in process memory. If you
  run multiple front-end servers, each tracks its own counters; an
  attacker only needs one of them to lock out before others. Pair
  with a load balancer that pins source IPs to backends, or use a
  shared backing store (out-of-scope for 1.x).
- **Not a CAPTCHA / proof-of-work / honeypot system.** Those are
  orthogonal anti-abuse measures; this is the simplest "n strikes
  and you're out" pattern.

## Performance

Measured (criterion, M-series Mac, release, 100-sample median):

| Operation | Median |
|---|---:|
| `check` — empty map (success path) | **43 ns** |
| `check` — below threshold, still allowed | **46 ns** |
| `check` — IP locked out | **51 ns** |
| `record_failure` — fresh (IP, username) key | **127 ns** |
| `record_failure` — repeat same key | **75 ns** |
| `record_success` — clears account counter | **62 ns** |

The success path (`check` → `Allowed`) is the hottest case — every
legitimate login goes through it. Two DashMap reads, no allocation,
~43 ns flat regardless of map size. Reproduce:
`cargo bench -p mailrs-auth-guard --bench guard`.

Workspace [PERFORMANCE.md](../../PERFORMANCE.md) carries the same
table.

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-auth-guard`) |
| **test** | line cov: 99.5% (`cargo llvm-cov -p mailrs-auth-guard --summary-only`) |
| **bench** | ✅ 1 file(s) criterion + ✅ 4 gate(s) `perf_gate.rs` |
| **size** | release rlib: 131 KB |
| **fuzz** | ❌ none |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons

- Searched crates.io + competing impls: see PERFORMANCE.md or 'first-in-Rust' marker.

<!-- AUDIT-FOOTER:END -->

## License

Apache-2.0 OR MIT.
