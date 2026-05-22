# mailrs-acme

[![Crates.io](https://img.shields.io/crates/v/mailrs-acme?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-acme)
[![docs.rs](https://img.shields.io/docsrs/mailrs-acme?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-acme)
[![License](https://img.shields.io/crates/l/mailrs-acme?style=flat-square)](#license)

High-level ACME (RFC 8555 / Let's Encrypt) orchestration. Wraps the
low-level [`instant-acme`](https://crates.io/crates/instant-acme)
client with the pieces every server needs:

- **account load-or-create** (persisted under `<acme_dir>/account.json`)
- **HTTP-01 cert provisioning** with a shared token store
- **expiry monitoring** via x509 parsing
- **periodic renewal task** that swaps new certs into
  [`mailrs-tls-reload`](https://crates.io/crates/mailrs-tls-reload)
  atomically (no in-flight handshakes dropped)
- **optional bundled axum HTTP-01 challenge server** (feature flag)

If you only want the protocol pieces, use `instant-acme` directly.
If you want "give me a TlsState that stays renewed forever," use this.

## Quickstart

```rust,no_run
use mailrs_acme::{init, new_challenge_tokens, spawn_challenge_server, spawn_renewal_task, RenewalConfig};
use std::path::Path;
use std::net::SocketAddr;
use tokio::sync::watch;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let (shutdown_tx, shutdown_rx) = watch::channel(false);
let tokens = new_challenge_tokens();

// Start the HTTP-01 challenge server on port 80 (or your reverse-proxy
// target). Skip this and serve the route from your own HTTP stack if
// you prefer — read tokens from the same map.
spawn_challenge_server(
    tokens.clone(),
    SocketAddr::from(([0, 0, 0, 0], 80)),
    shutdown_rx.clone(),
);

// Init: load existing cert if valid, else provision via HTTP-01.
let (tls_state, account) = init(
    "ops@example.com",
    &["example.com".into(), "www.example.com".into()],
    Path::new("/var/acme"),
    false,                    // production (not staging)
    &tokens,
).await?;

// Spawn the renewal task (12h check interval, renew at ≤30 days).
spawn_renewal_task(
    account,
    tokens.clone(),
    tls_state.clone(),
    RenewalConfig {
        domains: vec!["example.com".into(), "www.example.com".into()],
        acme_dir: "/var/acme".into(),
        ..Default::default()
    },
    shutdown_rx,
);

// Use `tls_state.acceptor()` in your TLS accept loop. It always
// returns the current (post-renewal) config.
# Ok(())
# }
```

## What this crate does

- `init(email, domains, acme_dir, staging, tokens) -> (TlsState, Account)`
  — one-shot bootstrapper. Loads or provisions a cert; returns
  the things you need to plug into your server.
- `load_or_create_account(email, staging, acme_dir)` — just the
  account step.
- `provision_cert(account, domains, tokens)` — just the cert flow.
- `cert_days_remaining(pem_bytes) -> i64` — x509 expiry helper.
- `save_cert(acme_dir, cert, key)` / `build_server_config(cert, key)`
  — file + rustls glue.
- `spawn_renewal_task(account, tokens, tls_state, config, shutdown)`
  — periodic check + auto-swap.
- `spawn_challenge_server(tokens, addr, shutdown)` — bundled axum
  HTTP-01 server (feature-gated `axum-http`, on by default).
- `new_challenge_tokens()` / `ChallengeTokens` type — shared
  token-store between provisioner + challenge server.

## What this crate does not

- **Not an ACME protocol implementation.** That's
  [`instant-acme`](https://crates.io/crates/instant-acme) — this
  crate calls into it.
- **No DNS-01 challenge.** HTTP-01 only in 1.0. (DNS-01 needs DNS
  provider integration which is per-provider; a future
  `mailrs-acme-dns01` could provide the orchestration layer for
  caller-supplied DNS clients.)
- **No TLS-ALPN-01 challenge.** Same reasoning — fits a future
  separate crate if needed.
- **No multi-account / wildcard / EAB-protected CA support.** All
  reachable via `instant-acme` directly; this crate is for the
  common "one account, one cert per cert-renewing server, public
  CA" path.
- **No file watcher for external renewals.** If certbot writes new
  PEMs to your `acme_dir`, you'd call `tls_state.swap(new_config)`
  from your own file-change handler.

## Features

| Feature | Default | What it adds |
|---|---|---|
| `axum-http` | ✓ | The bundled `spawn_challenge_server` (axum) |

Disable `default-features = false` if you serve the challenge from
your own HTTP stack (actix-web, warp, hyper, …). The
`ChallengeTokens` API stays available regardless.

## License

Apache-2.0 OR MIT.
