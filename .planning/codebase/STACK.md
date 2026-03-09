# Technology Stack

**Analysis Date:** 2026-03-09

## Languages

**Primary:**
- Rust (2021 edition) - Backend server, all protocol handling, mail processing (`crates/`)
- TypeScript (~5.9.3) - Web frontend (`web/src/`)

**Secondary:**
- SQL - Database schema and migrations (`scripts/init-schema.sql`, `scripts/migrate-*.sql`)
- Shell (Bash) - Build, deploy, and release scripts (`scripts/`)

## Runtime

**Backend:**
- Rust stable toolchain (rustc 1.94.0) - configured in `rust-toolchain.toml`
- Tokio 1.49.0 async runtime (full features)

**Frontend:**
- Bun 1.x - Package manager and dev tooling (NOT npm/yarn)
- Lockfile: `web/bun.lock` (present)

**Rust lockfile:** `Cargo.lock` (present, committed)

## Frameworks

**Core:**
- Axum 0.8.8 - HTTP/WebSocket server (`crates/server/Cargo.toml`)
- React 19.2.0 - Web UI (`web/package.json`)
- Vite 7.3.1 - Frontend build tool (`web/vite.config.ts`)

**Testing:**
- Rust built-in `#[test]` - Backend tests (`cargo test`)
- Vitest 3.2.1 - Frontend tests (`bun run test`)
- Testing Library (React 16.3.2, jest-dom 6.9.1, user-event 14.6.1) - Component testing

**Build/Dev:**
- cargo zigbuild - Cross-compilation for `aarch64-unknown-linux-gnu` deployment
- Vite 7.3.1 with `@vitejs/plugin-react` 5.1.1 - Frontend bundling
- Docker multi-stage build - Production container (`Dockerfile`)

## Key Dependencies

### Rust (Backend)

**Critical:**
- `sqlx` 0.8.6 - PostgreSQL async driver (runtime queries, not compile-time checked)
- `redis` 0.27 - Valkey/Redis client with tokio-comp and connection-manager
- `tokio-rustls` 0.26 / `rustls` 0.23 - TLS for SMTP/IMAP/SMTPS
- `axum` 0.8 (with `ws` and `multipart` features) - Web API and WebSocket
- `mail-parser` 0.10 - RFC 5322 email parsing
- `mail-auth` 0.7 - SPF/DKIM/DMARC verification and signing

**Infrastructure:**
- `hickory-resolver` 0.25 - DNS resolution (MX lookups, PTR, DNSBL)
- `instant-acme` 0.8 - Let's Encrypt ACME certificate automation
- `arc-swap` 1 - Hot-reloadable TLS certificates
- `dashmap` 6 - Concurrent in-process cache
- `sieve-rs` 0.7 - Sieve email filtering language
- `argon2` 0.5 / `password-hash` 0.5 - Password hashing
- `tower-http` 0.6 - CORS and static file serving
- `reqwest` 0.12 - HTTP client for AI API calls
- `tracing` 0.1 - Structured logging

**Content processing:**
- `html2text` 0.14 - HTML-to-text conversion for email bodies
- `pdf-extract` 0.7 - PDF text extraction from attachments
- `image` 0.25 - Image processing (PNG, JPEG, WebP, TIFF)
- `flate2` 1 - Gzip compression
- `dompurify` 3.3.1 (frontend) - HTML sanitization

### TypeScript (Frontend)

**Critical:**
- `jotai` 2.18.0 - Atomic state management
- `react-router` 7.13.1 - Client-side routing
- `@tiptap/*` 3.20.1 - Rich text editor (compose/reply)
- `lucide-react` 0.577.0 - Icon library
- `sonner` 2.0.7 - Toast notifications
- `react-markdown` 10.1.0 / `remark-gfm` 4.0.1 / `rehype-highlight` 7.0.2 - Markdown rendering
- `highlight.js` 11.11.1 / `lowlight` 3.3.0 - Syntax highlighting

**Dev tooling:**
- `tailwindcss` 4.2.1 (via `@tailwindcss/vite` plugin) - CSS utility framework
- `eslint` 9.39.1 with `typescript-eslint` 8.48.0 - Linting
- `jsdom` 26.1.0 - Test environment

## Configuration

**Environment:**
- All configuration via `MAILRS_*` environment variables - no config files except `users.toml`
- Config parsed in `crates/server/src/config.rs` via `ServerConfig::from_env()`
- `.env.local` file present (gitignored) for local development secrets

**Key env var groups:**
- `MAILRS_HOSTNAME`, `MAILRS_MAILDIR`, `MAILRS_LOCAL_DOMAINS` - Core identity
- `MAILRS_PORT`, `MAILRS_SUBMISSION_PORT`, `MAILRS_SMTPS_PORT`, `MAILRS_IMAP_PORT`, `MAILRS_WEB_PORT` - Ports
- `MAILRS_TLS_CERT`, `MAILRS_TLS_KEY` - Manual TLS
- `MAILRS_ACME_EMAIL`, `MAILRS_ACME_DOMAINS` - Auto TLS via Let's Encrypt
- `MAILRS_PG_URL`, `MAILRS_VALKEY_URL` - Storage backends (both optional)
- `MAILRS_DKIM_*` - DKIM signing
- `MAILRS_AI_*`, `MAILRS_GEMINI_API_KEY` - AI features

**Build:**
- `Cargo.toml` - Workspace root with shared dependency versions
- `web/vite.config.ts` - Frontend build config with `@` path alias to `src/`
- `rust-toolchain.toml` - Rust stable channel

## Platform Requirements

**Development:**
- Rust stable toolchain
- Bun 1.x
- Docker + Docker Compose (for PostgreSQL and Valkey)
- Local dev via `scripts/dev.sh` (SMTP on 2525, submission on 2587, IMAP on 1143, API on 3200, Vite on 5173)

**Production:**
- Debian Bookworm slim (Docker runtime image)
- `cargo zigbuild` for aarch64-unknown-linux-gnu cross-compilation
- Deployed via SSH/SCP to remote host (`scripts/deploy.sh`)
- Docker Compose orchestration on remote (postgres, valkey, mailrs containers)
- Ports exposed: 25 (SMTP), 80 (HTTP/ACME), 143 (IMAP), 465 (SMTPS), 587 (submission), 3100 (web)

**Workspace version:** 0.6.25 (synced between `Cargo.toml` and `web/package.json`)

---

*Stack analysis: 2026-03-09*
