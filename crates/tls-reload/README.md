# mailrs-tls-reload

[![Crates.io](https://img.shields.io/crates/v/mailrs-tls-reload?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-tls-reload)
[![docs.rs](https://img.shields.io/docsrs/mailrs-tls-reload?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-tls-reload)
[![License](https://img.shields.io/crates/l/mailrs-tls-reload?style=flat-square)](#license)

Hot-reloadable rustls `ServerConfig` via `arc-swap`. Drop-in helper
for any rustls-terminating server that needs to rotate certificates
without dropping in-flight connections.

```text
Renewal hook:   state.swap(new_config)
                          │
                          ▼
TlsState (Arc<ArcSwap<ServerConfig>>)
                          │
        ┌─────────────────┼─────────────────┐
        ▼                 ▼                 ▼
   acceptor() at      acceptor() at      acceptor() at
   t < swap           t == swap          t > swap
   (old config)       (atomic boundary)  (new config)
```

The pattern: hold one `TlsState` for the server's lifetime, derive a
fresh `TlsAcceptor` per accepted connection, swap the active config
from your renewal hook (ACME, certbot reload signal, kubernetes
secret update, …) when new PEMs land.

## Quickstart

```rust,no_run
use mailrs_tls_reload::{TlsState, load_tls_config};
use std::path::Path;

# async fn run() -> std::io::Result<()> {
// On startup:
let cfg = load_tls_config(Path::new("cert.pem"), Path::new("key.pem"))?;
let state = TlsState::new((*cfg).clone());

// In your accept loop:
let acceptor = state.acceptor();
// tokio::spawn(async move { ... acceptor.accept(socket).await ... });

// In your renewal hook (e.g. ACME post-renewal callback):
let new_cfg = load_tls_config(Path::new("cert.pem"), Path::new("key.pem"))?;
state.swap((*new_cfg).clone());
// New acceptor() calls now use new cert. Existing handshakes finish
// with the old cert (each acceptor() snapshotted the old pointer).
# Ok(())
# }
```

## What this crate does

- **`TlsState::new(config)`** — wrap a `rustls::ServerConfig` in
  `Arc<ArcSwap<_>>`
- **`TlsState::acceptor()`** — snapshot current config into a fresh
  `TlsAcceptor` (one atomic load, no contention with concurrent swaps)
- **`TlsState::swap(new_config)`** — atomically replace the active
  config; old in-flight handshakes are unaffected
- **`TlsState::current()`** — read the current config (`Arc` clone)
- **`load_tls_config(cert_path, key_path)`** — PEM file loader,
  returns ready-to-use `Arc<ServerConfig>`. No client auth, supports
  RSA + EC keys (PKCS#1 / PKCS#8 / SEC1).

That's the whole surface. ~50 LOC of library code.

## What this crate does not

- **Not a cert-renewal runtime.** You bring the renewal (ACME via
  `instant-acme` / `acme-client`, certbot, k8s cert-manager …); this
  crate is the "atomic-swap" mechanism.
- **Not a file-watcher.** If you want auto-reload on disk change, wrap
  with `notify` yourself — one fn call to `swap()` per change event.
- **No client-cert auth.** The PEM loader builds a no-client-auth
  config. Construct your own `ServerConfig` and hand it to
  `TlsState::new` if you need mTLS.
- **No ALPN selection.** rustls's `ServerConfig` exposes `alpn_protocols`
  directly; set it before handing the config to `TlsState`.

## Why arc-swap?

Three alternatives, all worse for this use case:

| Approach | Issue |
|---|---|
| `Mutex<ServerConfig>` | All `acceptor()` calls contend; renewal blocks accept loop |
| `RwLock<ServerConfig>` | Reader-heavy fine but acquires + releases a lock per accept |
| Channel + per-handler updates | Complicates accept-loop, harder to reason about transitions |

`ArcSwap` is **wait-free for readers** (one atomic load). Writers
(`swap`) are wait-free too. Standard pattern for read-mostly,
infrequently-updated shared state — exactly the certificate use case.

## Performance

`acceptor()` is one `ArcSwap::load_full` (one atomic + one clone of
the inner `Arc`). Cost is ~1-10 ns; never a contention point.

`swap()` is one atomic store; also wait-free.

No bench file — the perf is dominated by `arc_swap`'s internals,
which has its own perf coverage.

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-tls-reload`) |
| **test** | line cov: 98.6% (`cargo llvm-cov -p mailrs-tls-reload --summary-only`) |
| **bench** | ✅ 0 file(s) criterion + ❌ none `perf_gate.rs` |
| **size** | release rlib: 164 KB |
| **fuzz** | ❌ none |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons

- Searched crates.io + competing impls: see PERFORMANCE.md or 'first-in-Rust' marker.

<!-- AUDIT-FOOTER:END -->

## License

Apache-2.0 OR MIT.
