# Coding Conventions

**Analysis Date:** 2026-03-09

## Naming Patterns

**Rust Files:**
- `snake_case.rs` for all modules: `smtp_session.rs`, `domain_store.rs`, `html_clean.rs`
- Test files as submodules: `crates/smtp-proto/src/session/tests.rs`, `crates/storage-maildir/src/tests.rs`
- Inline test modules at file bottom with `#[cfg(test)] mod tests { ... }`

**Rust Crates:**
- Published names prefixed `mailrs-`: `mailrs-smtp-proto`, `mailrs-server`, `mailrs-mailbox`
- Directory names omit the prefix: `crates/smtp-proto`, `crates/server`, `crates/mailbox`
- Use `workspace = true` for shared version in crate `Cargo.toml`

**TypeScript Files:**
- `kebab-case.ts` / `kebab-case.tsx`: `email-split.ts`, `thread-view.tsx`, `use-mail-events.ts`
- Test files in `__tests__/` subdirectories: `web/src/lib/__tests__/format.test.ts`
- Hook files prefixed `use-`: `use-mail-events.ts`, `use-smtp-events.ts`, `use-keyboard-nav.ts`

**Rust Functions:**
- `snake_case` for all functions: `make_delivery_decision()`, `parse_command()`, `clean_email_html()`
- Public API uses `pub fn` or `pub async fn`; internal uses `pub(crate)` or `pub(super)`

**TypeScript Functions:**
- `camelCase` for functions and variables: `formatDate()`, `authHeaders()`, `handleResponse()`
- `PascalCase` for React components: `Button`, `ThreadView`, `ConversationList`
- `camelCase` for Jotai atoms with `Atom` suffix: `conversationsAtom`, `selectedThreadIdAtom`, `authAtom`

**Rust Types:**
- `PascalCase` for structs and enums: `ServerConfig`, `DeliveryDecision`, `SmtpEvent`
- Enum variants are `PascalCase`: `DeliveryDecision::Accept`, `State::Greeted`
- Constants are `UPPER_SNAKE_CASE`: `MAX_MESSAGE_SIZE`, `CONNECTION_TIMEOUT`, `SESSION_TTL`

**TypeScript Types:**
- `PascalCase` for types and interfaces: `ButtonVariant`, `ConversationSummary`, `ThreadMessage`
- Type exports colocated with implementation in same file
- Shared types centralized in `web/src/lib/types.ts`

## Code Style

**Formatting (Rust):**
- Standard `rustfmt` defaults (no `.rustfmt.toml` present)
- Stable toolchain via `rust-toolchain.toml`
- No Clippy config file; default Clippy lints apply

**Formatting (TypeScript):**
- No Prettier config; relies on ESLint for style enforcement
- ESLint flat config at `web/eslint.config.js`
- Plugins: `typescript-eslint`, `eslint-plugin-react-hooks`, `eslint-plugin-react-refresh`
- No trailing semicolons (observed throughout codebase)

**Linting (TypeScript):**
- Config: `web/eslint.config.js`
- Extends: `js.configs.recommended`, `tseslint.configs.recommended`, `reactHooks.configs.flat.recommended`
- Target: `**/*.{ts,tsx}` files only
- Run: `bun run lint`

## Import Organization

**Rust:**
1. `std::` imports first
2. External crate imports (`axum`, `tokio`, `serde`, `sqlx`, etc.)
3. Internal workspace crate imports (`mailrs_smtp_proto::`, `mailrs_storage_maildir::`)
4. Local `crate::` / `super::` imports last

Example from `crates/server/src/smtp_session.rs`:
```rust
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use hickory_resolver::TokioResolver;
use tokio::io::{AsyncRead, AsyncWrite};

use mailrs_smtp_proto::response::{format_ehlo_response, Response};
use mailrs_smtp_proto::session::{AuthStep, Event, Session, SessionConfig};

use crate::codec::{SmtpCodec, SmtpInput};
use crate::config::SmuggleProtection;
```

**TypeScript:**
1. Third-party imports (react, jotai, libraries)
2. Path-aliased internal imports (`@/store/`, `@/lib/`, `@/components/`)
3. Relative imports (same module)

**Path Aliases:**
- `@` maps to `web/src/` (configured in `web/vite.config.ts` as `{ '@': '/src' }`)

## Error Handling

**Rust Patterns:**

1. **Result<T, String> for internal functions** - Many functions return `Result<T, String>` with `format!()` error messages:
   ```rust
   pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, String> {
       let pkcs8 = PrivatePkcs8KeyDer::from_pem_slice(...)
           .map_err(|e| format!("failed to parse DKIM PEM: {e}"))?;
   ```

2. **Result<T, sqlx::Error> for database operations** - Direct sqlx error propagation:
   ```rust
   pub async fn create_mailbox(&self, user: &str, name: &str) -> Result<Mailbox, sqlx::Error> {
   ```

3. **Axum handler responses** - Return `(StatusCode, Json<ApiResult>)` tuples for web endpoints:
   ```rust
   (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "invalid input length"})))
   ```

4. **`ApiResult` struct** for uniform JSON responses in `crates/server/src/web/mod.rs`:
   ```rust
   struct ApiResult {
       success: bool,
       message: Option<String>,
   }
   ```

**TypeScript Patterns:**

1. **Centralized `handleResponse<T>`** in `web/src/lib/api.ts`:
   - 401 -> clear localStorage, redirect to `/login`
   - Non-OK -> attempt to parse error body, throw `Error` with message
   - Success -> parse JSON as `T`

2. **Error boundary** at app root via `web/src/components/error-boundary.tsx`

3. **Async errors** caught per-component (no global error store observed)

## Logging

**Rust:**
- Framework: `tracing` crate (version 0.1)
- Used primarily in server crate: `tracing::info!`, `tracing::warn!`, `tracing::error!`
- ~31 tracing macro calls across 8 files in `crates/server/src/`
- Structured logging with field syntax: `tracing::info!(id = conn_id, addr = %addr, "connection opened")`

**TypeScript:**
- `console` only (no structured logging library)
- Minimal client-side logging

## Comments

**Rust:**
- Doc comments (`///`) on public items: `/// parse a single SMTP command line (without trailing CRLF)`
- Inline comments lowercase, no period: `// stage 1: remove script, style, comments`
- Section markers in test files: `// --- directory initialization ---`, `// --- delivery ---`
- Module-level comments at file top: `// importance scoring engine: determines email value/priority`

**TypeScript:**
- Inline comments lowercase, no period: `// helper: convert a Date to a unix timestamp (seconds)`
- Design token file has explanatory comments: `// these map to CSS custom properties defined in index.css`
- Minimal JSDoc/TSDoc usage

## Function Design

**Rust:**
- Pure functions separated from I/O: e.g., `make_delivery_decision()` in `crates/server/src/inbound/pipeline.rs` is a pure function with all I/O in `run_pipeline()`
- Protocol crates (`smtp-proto`, `imap-proto`) contain zero I/O - pure parsing and state machines
- Session handlers in server crate own all async I/O
- Builder pattern for `WebState` with `with_*` methods:
  ```rust
  pub fn with_queue(mut self, pool: sqlx::PgPool) -> Self { ... }
  pub fn with_mailbox(mut self, store: Arc<MailboxStore>) -> Self { ... }
  ```

**TypeScript:**
- Small utility functions in `web/src/lib/` (e.g., `format.ts` is 75 lines with 4 exported functions)
- React components use `forwardRef` for UI primitives (`web/src/components/ui/button.tsx`)
- Hooks encapsulate WebSocket logic: `web/src/hooks/use-mail-events.ts`

## Module Design

**Rust Exports:**
- Crate root (`lib.rs`) re-exports key public types:
  ```rust
  pub use command::{AuthMechanism, Command, ForwardPath, Param, ReversePath};
  pub use parse::{parse_command, ParseError};
  ```
- Visibility scoped with `pub(crate)` and `pub(super)` to prevent leaking internals

**TypeScript Exports:**
- Barrel file for UI components at `web/src/components/ui/index.ts`
- Named exports throughout (no default exports observed)
- Jotai atoms exported individually from store files

## Constants & Configuration

**Rust:**
- Constants defined with `const` at module top: `const SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 3600);`
- Validation limits as module-level constants in `crates/server/src/web/mod.rs`:
  ```rust
  const MAX_LIMIT: u32 = 100;
  const MAX_OFFSET: u32 = 1_000_000;
  const MAX_QUERY_LEN: usize = 500;
  const MAX_BATCH_SIZE: usize = 100;
  ```
- All server configuration via `MAILRS_*` environment variables (parsed in `crates/server/src/config.rs`)

**TypeScript:**
- Design tokens in `web/src/lib/tokens.ts` with `as const` assertions
- CSS custom properties (`--color-*`) for theming
- Local storage keys as module-level constants: `const STORAGE_KEY = 'mailrs_auth'`

## State Management (Frontend)

- **Jotai** for all global state
- Atom-per-concern pattern: `web/src/store/auth.ts`, `web/src/store/chat.ts`, `web/src/store/settings.ts`, `web/src/store/theme.ts`
- Derived atoms using `atom((get) => ...)`: e.g., `unreadCountAtom` derives from `conversationsAtom`
- No Redux, no Context API for state

## Serde Patterns (Rust)

- Derive `Serialize`/`Deserialize` on all API types
- `#[serde(skip_serializing_if = "Option::is_none")]` for optional fields
- `#[serde(default)]` for request fields with defaults
- `#[serde(tag = "type")]` for tagged enums (e.g., `SmtpEvent`)

## Input Validation (Web API)

- Length checks at handler entry: `if req.address.len() > MAX_ADMIN_FIELD_LEN`
- Pagination bounds: `MAX_LIMIT`, `MAX_OFFSET` enforced in query params
- Request body size limits: `MAX_MULTIPART_BODY = 25 MB`

---

*Convention analysis: 2026-03-09*
