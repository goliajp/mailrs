# mailrs

A mail server written in Rust, with a React web client and iOS and Android
apps. It handles SMTP in and out, IMAP, POP3, JMAP, ManageSieve and
CalDAV/CardDAV, and exposes a REST API and an MCP server for agents.

## Architecture

Production runs four processes from one image beside a `kevy-server`:

| process | role |
|---|---|
| `mailrs-receiver` | SMTP on 25, 465 and 587; the inbound pipeline (rate limits, DNSBL, greylisting, SPF/DKIM/DMARC/ARC, Bayes, fraud rules); writes accepted mail to a spool |
| `mailrs-fastcore` | Owns the mailbox: drains the spool, applies Sieve, delivers to Maildir, threads and indexes; IMAP, POP3 and ManageSieve; the core API the others call |
| `mailrs-webapi` | REST, JMAP, CalDAV/CardDAV, OIDC sign-in, MCP and the realtime WebSocket; serves the web client |
| `mailrs-fastcore-sender` | Outbound delivery: DKIM signing, MX and STARTTLS, MTA-STS, retries and DSNs |

There is no SQL database in that stack. Message metadata lives in an
embedded [kevy](https://github.com/goliajp/kevy) store inside fastcore,
state the processes share lives in `kevy-server`, and mail bodies are
Maildir files.

The workspace has 71 crates. Most of them are libraries that each own one
RFC or one concept — SMTP, IMAP, MIME, SPF, DKIM, DMARC, ARC, Sieve,
iCalendar, JMAP, MTA-STS and more — published on
[crates.io](https://crates.io/users/golia-jp) and usable on their own.
Protocol parsers do no I/O; the process crates own all networking.

[ARCHITECTURE.md](./ARCHITECTURE.md) describes the processes, where each
kind of state lives, how mail moves between them, and every crate.

### Performance

Measured head to head against other implementations (full ledger in
[PERFORMANCE.md](./PERFORMANCE.md), cross-language runners in
`bench-harness/`):

| scenario | mailrs | other | |
|---|---:|---:|---|
| RFC 5322 message scan + Subject + From | 46 ns | Go `net/mail`: 1440 ns | 31× |
| iCalendar parse | 1.76 µs | C `libical` 4.0: 7.03 µs | 4× |
| SMTP `EHLO` parse | 18 ns | Rust `smtp-codec`: 126 ns | 7× |
| DKIM-Signature parse | 405 ns | Rust `mail-auth` 0.9: 423 ns | +4% |
| Rate-limit hot-key check | 13–16 ns | Rust `governor` 0.10: 14–18 ns | tied |

Latency-sensitive crates carry `tests/perf_gate.rs` budgets that fail the
test suite on regression. Parsers of untrusted input have libFuzzer
targets in their `fuzz/` directories.

## Features

- **SMTP** — inbound and outbound, STARTTLS and implicit TLS, DKIM signing, SPF/DKIM/DMARC/ARC verification, DSNs; reads incoming DMARC aggregate and TLS-RPT reports and sends TLS-RPT reports
- **IMAP** — with IDLE, CONDSTORE and Dovecot-compatible UID and keyword files
- **POP3**, **JMAP** (RFC 8620/8621), **ManageSieve** (RFC 5804), **CalDAV/CardDAV**
- **Sieve** — server-side filtering
- **Anti-abuse** — greylisting, DNSBL, rate limits, login lockout, Bayesian spam filtering, inbox/notification/promotion triage, scripted fraud rules that can change without a release
- **Search** — full text over headers, bodies and attachment text, including OCR of images and scanned PDFs; Chinese and Japanese searchable without a dictionary
- **Web client** — conversation view with realtime updates; React 19, TypeScript, Vite, Tailwind CSS 4, TanStack Query
- **Apps** — native iOS and Android clients; new mail is announced through APNs on iOS and by a background check on Android
- **MCP** — 91 tools for agents to read, triage and send mail
- **Accounts** — Argon2 passwords, TOTP two-factor sign-in, OIDC
- **AI assistance** — optional, off unless an OpenAI-compatible endpoint is configured (`MAILRS_AI_ANALYSIS_ENABLED`, `MAILRS_LLM_URL`)

## Running it

### The reference deployment

`deploy/docker-compose.prod.yml` is the deployment mailrs itself runs. It
starts the four processes and `kevy-server` from
`ghcr.io/goliajp/mailrs:$MAILRS_VERSION`, and expects on the host:

- `.env` with `MAILRS_VERSION` and `MAILRS_CORE_API_SECRET` (the shared
  secret between the processes), plus the DKIM and SRS settings you use;
- `/apps/mailrs/certs` — the TLS certificate and key for SMTP, IMAP and POP3;
- `/apps/mailrs/dkim` — DKIM private keys;
- `/apps/mailrs/secrets` — the APNs key, if iOS push is used;
- `/apps/mailrs/fraud-rules/active.lua` — the fraud rule set;
- `/apps/mailrs/web` — the built web client (`cd web && bun run build`);
- `/apps/mailrs/kevy.toml` — the `kevy-server` configuration (`deploy/kevy.toml`).

Domains, hostnames and DKIM identities in that file are the project's own;
replace them with yours. The image is built from the root `Dockerfile`.

| port | service |
|---|---|
| 25 | SMTP |
| 465 | SMTPS |
| 587 | SMTP submission |
| 143 / 993 | IMAP / IMAPS |
| 110 / 995 | POP3 / POP3S |
| 4190 | ManageSieve |
| 3100 | web client, REST, JMAP, DAV, MCP (put a TLS proxy in front) |

### Development

```bash
cargo build
cargo test --workspace --no-fail-fast

# the web client (requires Bun)
cd web && bun install && bun run dev
```

`./scripts/local-fastcore-smoke.sh` boots `mailrs-fastcore` against a
scratch kevy directory, imports a small sample and probes its routes — the
production shape without Docker.

## Configuration

Every setting is a `MAILRS_*` environment variable. The ones a deployment
sets first:

| variable | meaning |
|---|---|
| `MAILRS_HOSTNAME` | The server's hostname (its MX name) |
| `MAILRS_LOCAL_DOMAINS` | Comma-separated domains this server receives mail for |
| `MAILRS_KEVY_URL` | The shared `kevy-server`, e.g. `kevy://kevy-server:6379` |
| `MAILRS_KEVY_DATA_DIR` | fastcore's embedded store |
| `MAILRS_MAILDIR` | Root of the Maildir tree |
| `MAILRS_SPOOL_ROOT` | The inbound spool shared by receiver and fastcore |
| `MAILRS_CORE_RPC_BASE`, `MAILRS_CORE_API_SECRET` | Where the other processes reach fastcore, and the secret they share |
| `MAILRS_TLS_CERT`, `MAILRS_TLS_KEY` | TLS certificate and key, reloaded when the files change |
| `MAILRS_DKIM_DOMAIN`, `MAILRS_DKIM_SELECTOR`, `MAILRS_DKIM_KEYS` | Outbound DKIM signing |

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.

© 2026 GOLIA K.K.
