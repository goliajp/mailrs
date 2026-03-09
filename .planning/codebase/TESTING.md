# Testing Patterns

**Analysis Date:** 2026-03-09

## Test Frameworks

**Rust:**
- Runner: Built-in `cargo test` (stable toolchain)
- No external test framework; uses `#[test]`, `#[tokio::test]`, `assert!`, `assert_eq!`, `assert!(matches!(..))`
- Dev dependencies: `tempfile` (3), `filetime` (0.2) in `crates/storage-maildir`

**TypeScript:**
- Runner: Vitest 3.2.1
- Config: Inline in `web/vite.config.ts` (`test: { environment: 'jsdom' }`)
- Assertion: Vitest built-in `expect`
- DOM testing: `@testing-library/react` 16.3.2, `@testing-library/user-event` 14.6.1, `@testing-library/jest-dom` 6.9.1
- DOM environment: `jsdom` 26.1.0

**Run Commands:**
```bash
cargo test                              # all Rust tests
cargo test -p mailrs-smtp-proto         # single crate
cargo test -p mailrs-storage-maildir    # single crate (has integration tests)
cd web && bun run test                  # all frontend tests (vitest run)
```

## Test File Organization

**Rust - Inline test modules (primary pattern):**
- Tests live at the bottom of the source file in a `#[cfg(test)] mod tests { ... }` block
- 65 files contain inline test modules across all crates
- Example: `crates/server/src/inbound/pipeline.rs` has `mod tests` at line 443

**Rust - Separate test files (for large test suites):**
- `crates/smtp-proto/src/session/tests.rs` (1264 lines) - separate file, imported via `mod tests;` in parent
- `crates/smtp-proto/src/parse/tests.rs` (768 lines)
- `crates/smtp-proto/src/address/tests.rs`, `crates/smtp-proto/src/auth/tests.rs`, `crates/smtp-proto/src/data/tests.rs`, `crates/smtp-proto/src/response/tests.rs`
- `crates/storage-maildir/src/tests.rs` (736 lines)

**Rust - Integration tests:**
- `crates/server/tests/e2e.rs` (1634 lines) - end-to-end SMTP session tests over TCP

**TypeScript - `__tests__/` directories:**
- Tests in `__tests__/` subdirectory adjacent to source:
  ```
  web/src/components/ui/__tests__/button.test.tsx
  web/src/components/__tests__/thread-view.test.tsx
  web/src/lib/__tests__/format.test.ts
  web/src/store/__tests__/auth.test.ts
  ```
- Naming: `{module-name}.test.ts` or `{component-name}.test.tsx`
- 29 frontend test files total

## Rust Test Structure

**Inline module pattern:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    // helper functions for test setup
    fn default_input() -> PipelineInput {
        PipelineInput {
            greylisted: false,
            auth: default_auth(),
            virus_found: None,
            // ...
        }
    }

    #[test]
    fn all_pass_accepts() {
        let d = make_delivery_decision(&default_input());
        assert!(matches!(d, DeliveryDecision::Accept { .. }));
    }

    #[test]
    fn dmarc_reject_returns_550() {
        let input = PipelineInput {
            auth: AuthResults {
                dmarc_policy: DmarcPolicy::Reject,
                ..default_auth()
            },
            ..default_input()
        };
        match make_delivery_decision(&input) {
            DeliveryDecision::Reject { code, message } => {
                assert_eq!(code, 550);
                assert!(message.contains("DMARC"));
            }
            other => panic!("expected Reject, got {other:?}"),
        }
    }
}
```

**Key patterns:**
- Helper functions (`default_input()`, `config()`, `session()`) build test fixtures at module top
- Struct update syntax (`..default_input()`) for creating variations
- `assert!(matches!(expr, Pattern { .. }))` for enum variant assertions
- `match` + `panic!("expected X, got {other:?}")` for detailed enum field assertions
- Section comments to group related tests: `// --- normal flow ---`, `// --- delivery ---`

**Async tests (e2e):**
```rust
#[tokio::test]
async fn basic_smtp_session() {
    let port = start_server().await;
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).await.unwrap();
    // ...
}
```

**Env-dependent tests with mutex:**
```rust
static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn config_from_env() {
    let _guard = ENV_LOCK.lock().unwrap();
    clear_mailrs_env();
    std::env::set_var("MAILRS_HOSTNAME", "test.local");
    // ...
}
```

## TypeScript Test Structure

**Component test pattern:**
```typescript
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { Button } from '../button'

afterEach(cleanup)

describe('Button', () => {
  it('renders children', () => {
    render(<Button>Click me</Button>)
    expect(screen.getByText('Click me')).toBeDefined()
  })

  it('handles click', () => {
    const onClick = vi.fn()
    render(<Button onClick={onClick}>Click</Button>)
    fireEvent.click(screen.getByRole('button'))
    expect(onClick).toHaveBeenCalledTimes(1)
  })
})
```

**Utility/library test pattern:**
```typescript
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { formatDate, formatSize } from '../format'

describe('formatDate', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(2024, 2, 6, 12, 0, 0))
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('shows HH:MM for today', () => {
    const result = formatDate(toTs(thisMorning))
    expect(result).toMatch(/\d{1,2}:\d{2}/)
  })
})
```

**Store test pattern (Jotai):**
```typescript
import { createStore } from 'jotai/vanilla'

describe('authAtom', () => {
  let mockStorage: Storage

  beforeEach(() => {
    mockStorage = makeLocalStorageMock()
    vi.stubGlobal('localStorage', mockStorage)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('reads null when localStorage is empty', () => {
    const store = createStore()
    expect(store.get(authAtom)).toBeNull()
  })
})
```

## Mocking

**TypeScript - `vi.mock()` for modules:**
```typescript
vi.mock('@/store/auth', () => ({
  getToken: vi.fn(),
}))

import { getToken } from '@/store/auth'
const mockGetToken = vi.mocked(getToken)
```

**TypeScript - `vi.stubGlobal()` for browser APIs:**
```typescript
vi.stubGlobal('fetch', makeFetchMock(200, { ok: true }))
vi.stubGlobal('localStorage', makeLocalStorageMock())

// cleanup in afterEach
afterEach(() => {
  vi.unstubAllGlobals()
})
```

**TypeScript - Component mocking pattern:**
```typescript
vi.mock('@/components/ai-analysis', () => ({
  AiAnalysisPanel: ({ message }: { message: { summary?: string } }) => (
    <div data-testid="ai-analysis">{message.summary}</div>
  ),
}))
```

**TypeScript - What to mock:**
- `fetch` (via `vi.stubGlobal`)
- `localStorage` (via `vi.stubGlobal`)
- API module (`@/lib/api`) when testing components
- Child components with complex dependencies
- `Element.prototype.scrollIntoView` (not implemented in jsdom)
- Toast libraries (`sonner`)

**Rust - No mocking framework:**
- Pure functions used for testability (e.g., `make_delivery_decision()` is pure; I/O in `run_pipeline()`)
- `tempfile::TempDir` for filesystem tests
- Real TCP connections in e2e tests (`crates/server/tests/e2e.rs`)
- Test-specific helpers that mirror production constructors: `store_from_toml()` in `crates/server/src/users.rs`

## Fixtures and Factories

**Rust:**
- Helper functions at top of test module serve as fixtures:
  ```rust
  fn config() -> SessionConfig {
      SessionConfig {
          tls_available: true,
          tls_active: false,
          require_tls_for_auth: true,
          max_size: MAX_MESSAGE_SIZE,
          max_recipients: MAX_RECIPIENTS,
      }
  }

  fn session() -> Session {
      Session::new("mx.test.local", config())
  }
  ```
- Composable helpers that build on each other:
  ```rust
  fn greeted(s: &mut Session) { s.handle_command(&Command::Ehlo("client.test")); }
  fn mail_from(s: &mut Session) { greeted(s); s.handle_command(&Command::MailFrom { ... }); }
  fn rcpt_to(s: &mut Session) { mail_from(s); s.handle_command(&Command::RcptTo { ... }); }
  ```

**TypeScript:**
- Inline sample data objects:
  ```typescript
  const sampleAuth: AuthInfo = {
    token: 'tok-abc123',
    address: 'user@example.com',
    display_name: 'Test User',
    super_domains: ['example.com'],
  }
  ```
- Factory functions for mocks:
  ```typescript
  function makeFetchMock(status: number, body: unknown, isJson = true): typeof fetch { ... }
  function makeLocalStorageMock(): Storage { ... }
  ```
- No shared fixture directory; fixtures are local to each test file

## Coverage

**Requirements:** No enforced coverage threshold
**No coverage tool configured** in either Rust or TypeScript configs

**View coverage (if needed):**
```bash
cargo install cargo-tarpaulin && cargo tarpaulin     # Rust
cd web && bunx vitest run --coverage                  # TypeScript
```

## Test Types

**Unit Tests (majority):**
- Rust: Inline `#[cfg(test)]` modules testing pure functions, parsers, state machines
- TypeScript: `__tests__/*.test.ts` testing utilities, atoms, components in isolation
- Test names are descriptive snake_case (Rust) or sentence strings (TS)

**Integration Tests:**
- `crates/storage-maildir/src/tests.rs` - filesystem integration (uses real tmpdir)
- `crates/server/tests/e2e.rs` - full SMTP session over TCP with inline test server
- TypeScript component tests with Jotai store + mocked API (e.g., `thread-view.test.tsx`)

**E2E Tests:**
- `crates/server/tests/e2e.rs` starts a minimal SMTP server on a random port, connects via TCP, and runs full SMTP conversations
- No browser-based E2E tests (no Playwright/Cypress)

## Common Patterns

**Async Testing (Rust):**
```rust
#[tokio::test]
async fn basic_session() {
    let port = start_server().await;
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).await.unwrap();
    let mut reader = BufReader::new(&mut stream);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    assert!(line.starts_with("220 "));
}
```

**Error Testing (Rust):**
```rust
#[test]
fn parse_empty_input_returns_incomplete() {
    assert_eq!(parse_command(""), Err(ParseError::Incomplete));
}

#[test]
fn unknown_command_returns_error() {
    assert_eq!(parse_command("XYZZY"), Err(ParseError::UnknownCommand));
}
```

**Timer Testing (TypeScript):**
```typescript
beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(new Date(2024, 2, 6, 12, 0, 0))
})
afterEach(() => {
  vi.useRealTimers()
})
```

**DOM Cleanup (TypeScript):**
```typescript
afterEach(cleanup)  // @testing-library/react cleanup
afterEach(() => { vi.unstubAllGlobals() })  // vitest global cleanup
```

## Test Crate Coverage by Module

| Crate | Files with tests | Notable test areas |
|-------|-----------------|-------------------|
| `smtp-proto` | 6 inline + 6 separate | Command parsing, session state machine, address parsing, auth |
| `storage-maildir` | 1 inline + 1 separate | Directory creation, message delivery, flags, entries listing |
| `server` | 30+ inline + 1 e2e | Config, pipeline decisions, DKIM, DMARC, sieve, web handlers |
| `imap-proto` | 3 inline | Command parsing, sequence sets, response formatting |
| `outbound-queue` | 6 inline | Retry logic, DSN generation, DKIM signing, MTA-STS |
| `mailbox` | 3 inline | Threading, store operations, type conversions |
| `smtp-client` | 3 inline | MX resolution, response parsing, connection handling |
| Web frontend | 29 test files | UI components, API client, stores, utilities, format helpers |

## Release Testing Workflow

The release script (`scripts/release.sh`) runs the full test suite before any deployment:
1. `cargo test --workspace` - all Rust tests
2. `cd web && bun run test` - all frontend tests (vitest)
3. Only proceeds to version bump and deploy if all tests pass

---

*Testing analysis: 2026-03-09*
