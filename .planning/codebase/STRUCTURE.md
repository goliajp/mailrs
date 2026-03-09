# Codebase Structure

**Analysis Date:** 2026-03-09

## Directory Layout

```
mailrs/
├── crates/                    # Rust workspace crates
│   ├── server/                # Main binary: wires all subsystems
│   │   ├── src/
│   │   │   ├── main.rs        # Entry point: config, listeners, workers
│   │   │   ├── smtp_session.rs    # SMTP connection handler
│   │   │   ├── imap_session.rs    # IMAP connection handler
│   │   │   ├── imap_codec.rs      # IMAP framing codec
│   │   │   ├── imap_format.rs     # IMAP response formatting
│   │   │   ├── config.rs          # MAILRS_* env var parsing
│   │   │   ├── event_bus.rs       # Tokio broadcast event system
│   │   │   ├── domain_store.rs    # Domain/account resolution (3-tier cache)
│   │   │   ├── domain_check.rs    # DNS health checks for domains
│   │   │   ├── codec.rs           # SMTP framing codec
│   │   │   ├── tls.rs             # TLS/STARTTLS with hot-reload (arc-swap)
│   │   │   ├── acme.rs            # Let's Encrypt automation
│   │   │   ├── users.rs           # File-based auth (users.toml)
│   │   │   ├── pg.rs              # PG pool creation
│   │   │   ├── valkey_store.rs    # Valkey/Redis connection
│   │   │   ├── health.rs          # Health check state
│   │   │   ├── sieve.rs           # Sieve mail filtering
│   │   │   ├── ai_analyzer.rs     # Background AI analysis worker
│   │   │   ├── ai_email.rs        # Gemini API client for analysis
│   │   │   ├── ai_spam.rs         # AI-based spam classification
│   │   │   ├── content_extract.rs # OCR/PDF text extraction
│   │   │   ├── content_worker.rs  # Background content extraction
│   │   │   ├── html_clean.rs      # HTML sanitization
│   │   │   ├── importance.rs      # Email importance scoring
│   │   │   ├── inline_image.rs    # Inline image handling
│   │   │   ├── message_util.rs    # Message parsing utilities
│   │   │   ├── structured_data.rs # Structured data extraction
│   │   │   ├── dmarc_report.rs    # DMARC aggregate report generation
│   │   │   ├── ptr_check.rs       # PTR record verification
│   │   │   ├── inbound/           # Inbound email pipeline
│   │   │   │   ├── mod.rs
│   │   │   │   ├── pipeline.rs    # Multi-stage delivery decision
│   │   │   │   ├── rate_limit.rs  # SMTP rate limiting (token bucket)
│   │   │   │   ├── auth_guard.rs  # Brute force protection
│   │   │   │   ├── auth_results.rs    # SPF/DKIM/DMARC result formatting
│   │   │   │   ├── content_scan.rs    # Content rule matching
│   │   │   │   ├── dnsbl.rs       # DNS blocklist checks
│   │   │   │   ├── greylist_db.rs # Greylist triplet storage
│   │   │   │   └── greylisting.rs # Greylist decision logic
│   │   │   └── web/               # REST API + WebSocket
│   │   │       ├── mod.rs         # Router, WebState, shared types
│   │   │       ├── auth.rs        # Login/logout/me endpoints
│   │   │       ├── mail.rs        # CRUD mail, send, attachments, drafts
│   │   │       ├── conversations.rs   # Thread-based conversation API
│   │   │       ├── admin.rs       # Domain/account/alias management
│   │   │       ├── ai_assist.rs   # AI polish/reply-suggest endpoints
│   │   │       ├── autodiscover.rs    # Outlook/Mozilla autoconfig
│   │   │       ├── templates.rs   # Email template CRUD
│   │   │       ├── ws.rs          # WebSocket event streaming
│   │   │       ├── rate_limit.rs  # Web API rate limiting middleware
│   │   │       └── request_id.rs  # Request ID middleware
│   │   └── tests/                 # Integration tests
│   ├── smtp-proto/            # Pure SMTP protocol parser
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── command.rs     # Command enum + parsing
│   │       ├── address/       # Email address parsing
│   │       ├── auth/          # SMTP AUTH mechanisms
│   │       ├── data/          # DATA dot-stuffing
│   │       ├── parse/         # Command parser
│   │       ├── response/      # Response formatting
│   │       └── session/       # State machine (Session, State, Event)
│   ├── imap-proto/            # Pure IMAP protocol parser
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── command.rs     # IMAP command parser (1749 lines)
│   │       ├── response.rs    # Response formatting
│   │       └── sequence.rs    # Sequence set parsing
│   ├── smtp-client/           # Outbound SMTP connections
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── connection.rs  # SmtpConnection (TLS, EHLO, AUTH, send)
│   │       ├── mx.rs          # MX record resolution + caching
│   │       └── response.rs    # SMTP response parsing
│   ├── outbound-queue/        # Delivery queue with retry/bounce
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── queue.rs       # Queue operations (PG-backed)
│   │       ├── worker.rs      # DeliveryWorker (poll, deliver, retry)
│   │       ├── retry.rs       # Exponential backoff logic
│   │       ├── dkim_sign.rs   # DKIM signature generation
│   │       ├── dsn.rs         # Delivery Status Notification (bounce)
│   │       └── mta_sts.rs     # MTA-STS policy fetching
│   ├── mailbox/               # Mailbox metadata (PG-backed)
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── store.rs       # MailboxStore (2414 lines, main query hub)
│   │       ├── threading.rs   # Message threading (References/In-Reply-To)
│   │       └── types.rs       # Mailbox, MessageMeta, ConversationSummary
│   └── storage-maildir/       # Maildir file format
│       └── src/
│           ├── lib.rs         # Maildir struct, read/write/flag operations
│           └── tests.rs       # Integration tests
├── web/                       # React SPA frontend
│   ├── src/
│   │   ├── main.tsx           # React DOM mount
│   │   ├── app.tsx            # Routes + auth guard
│   │   ├── index.css          # Tailwind CSS entry
│   │   ├── pages/             # Route-level components
│   │   │   ├── chat.tsx       # Main conversation view (default route)
│   │   │   ├── login.tsx      # Login form
│   │   │   ├── admin.tsx      # Admin panel layout
│   │   │   ├── admin-overview.tsx
│   │   │   ├── admin-domains.tsx
│   │   │   ├── admin-accounts.tsx
│   │   │   ├── admin-aliases.tsx
│   │   │   ├── admin-queues.tsx
│   │   │   ├── protocol.tsx   # SMTP protocol browser
│   │   │   ├── settings.tsx   # User settings
│   │   │   └── playground.tsx # UI component playground
│   │   ├── components/        # Reusable UI components
│   │   │   ├── conversation-list.tsx  # Left panel conversation list
│   │   │   ├── thread-view.tsx        # Right panel thread detail
│   │   │   ├── message-bubble.tsx     # Individual message display
│   │   │   ├── reply-box.tsx          # Reply/compose form
│   │   │   ├── new-conversation.tsx   # New email compose
│   │   │   ├── rich-editor.tsx        # Rich text editor
│   │   │   ├── markdown-editor.tsx    # Markdown editor
│   │   │   ├── app-sidebar.tsx        # Main navigation sidebar
│   │   │   ├── admin-sidebar.tsx      # Admin navigation sidebar
│   │   │   ├── ai-analysis.tsx        # AI analysis display
│   │   │   ├── attachment-preview.tsx # Attachment viewer
│   │   │   ├── category-badge.tsx     # Email category badge
│   │   │   ├── contact-autocomplete.tsx
│   │   │   ├── context-menu.tsx       # Right-click menu
│   │   │   ├── copy-button.tsx
│   │   │   ├── domain-health-card.tsx
│   │   │   ├── error-boundary.tsx
│   │   │   ├── keyboard-shortcuts-dialog.tsx
│   │   │   ├── structured-data-card.tsx
│   │   │   ├── ui/            # Design system primitives
│   │   │   │   └── __tests__/
│   │   │   └── __tests__/     # Component tests
│   │   ├── hooks/             # Custom React hooks
│   │   │   ├── use-mail-events.ts     # WebSocket + polling for new mail
│   │   │   ├── use-smtp-events.ts     # WebSocket for protocol browser
│   │   │   └── use-keyboard-nav.ts    # Keyboard navigation
│   │   ├── store/             # Jotai atom state management
│   │   │   ├── auth.ts        # Auth token + user state
│   │   │   ├── chat.ts        # Conversations, threads, filters, selection
│   │   │   ├── admin.ts       # Admin panel state
│   │   │   ├── settings.ts    # User preferences
│   │   │   ├── theme.ts       # Theme state
│   │   │   └── __tests__/     # Store tests
│   │   ├── lib/               # Shared utilities
│   │   │   ├── api.ts         # HTTP client (fetchJson, postJson, etc.)
│   │   │   ├── types.ts       # TypeScript type definitions
│   │   │   ├── format.ts      # Date/size formatting
│   │   │   ├── tokens.ts      # Design token definitions
│   │   │   ├── theme.ts       # Theme utilities
│   │   │   ├── avatar.ts      # Avatar generation
│   │   │   ├── email-split.ts # Email address parsing
│   │   │   ├── mention.tsx    # @mention component
│   │   │   ├── notification-sound.ts
│   │   │   └── __tests__/     # Utility tests
│   │   └── assets/            # Static assets (images, fonts)
│   ├── public/                # Public static files
│   ├── dist/                  # Production build output
│   ├── package.json
│   ├── tsconfig.json
│   ├── vite.config.ts
│   └── tailwind.config.ts
├── scripts/                   # Build, deploy, migration scripts
│   ├── dev.sh                 # Local dev runner (cargo + vite)
│   ├── release.sh             # Full release pipeline
│   ├── deploy.sh              # Deploy to production
│   ├── dist.sh                # Cross-compile + package
│   ├── bump.sh                # Version bump
│   ├── init-schema.sql        # Initial PG schema
│   ├── migrate-001-email-analysis.sql
│   ├── migrate-002-clean-text.sql
│   ├── migrate-002-supermode.sql
│   ├── migrate-003-drafts.sql
│   ├── migrate-004-pinned.sql
│   ├── migrate-005-archive.sql
│   ├── migrate-006-indexes.sql
│   ├── migrate-007-attachment-content.sql
│   ├── migrate-008-email-templates.sql
│   ├── migrate-009-contacts-and-importance.sql
│   ├── import-samples.py      # Email import from IMAP
│   ├── import-remote.py       # Remote import
│   └── fix-maildir.py         # Maildir repair tool
├── Cargo.toml                 # Workspace root manifest
├── Cargo.lock
├── Dockerfile                 # Multi-stage (Rust + Bun + Debian slim)
├── docker-compose.yml         # postgres + valkey + mailrs services
├── users.toml                 # File-based user credentials
├── rust-toolchain.toml        # Rust toolchain pinning
├── dist/                      # Cross-compiled release artifacts
└── samples/                   # Sample .eml files (gitignored)
```

## Directory Purposes

**`crates/server/`:**
- Purpose: Main application binary that wires all other crates
- Contains: Protocol session handlers, web API, inbound pipeline, configuration, background workers
- Key files: `src/main.rs` (791 lines, bootstrap), `src/web/mod.rs` (1108 lines, router + state)

**`crates/smtp-proto/`:**
- Purpose: Pure SMTP command/response parsing and session state machine
- Contains: Parser, command enums, response formatters, session FSM
- Key files: `src/session/` (state machine), `src/command.rs` (command types)

**`crates/imap-proto/`:**
- Purpose: Pure IMAP command parsing and response formatting
- Contains: Command parser, response helpers, sequence set operations
- Key files: `src/command.rs` (1749 lines, full IMAP command parser)

**`crates/smtp-client/`:**
- Purpose: Outbound SMTP connection management
- Contains: TLS connections, MX resolution with caching, response parsing
- Key files: `src/connection.rs`, `src/mx.rs`

**`crates/outbound-queue/`:**
- Purpose: PG-backed delivery queue with retry/bounce logic
- Contains: Queue CRUD, delivery worker, exponential backoff, DKIM signing, DSN, MTA-STS
- Key files: `src/worker.rs` (775 lines), `src/queue.rs`

**`crates/mailbox/`:**
- Purpose: PG-backed mailbox metadata and conversation queries
- Contains: Mailbox CRUD, message threading, conversation aggregation, search
- Key files: `src/store.rs` (2414 lines, primary query hub), `src/threading.rs`

**`crates/storage-maildir/`:**
- Purpose: Maildir file format operations
- Contains: File read/write, flag management, message ID generation
- Key files: `src/lib.rs`

**`web/src/pages/`:**
- Purpose: Route-level page components
- Contains: Full-page views corresponding to React Router routes
- Key files: `chat.tsx` (main email view), `admin.tsx` (admin panel)

**`web/src/components/`:**
- Purpose: Reusable UI components
- Contains: Conversation list, thread view, message bubbles, editors, navigation
- Key files: `conversation-list.tsx`, `thread-view.tsx`, `message-bubble.tsx`

**`web/src/store/`:**
- Purpose: Jotai atom state management
- Contains: Auth state, chat/conversation state, admin state, settings, theme
- Key files: `chat.ts` (most atoms), `auth.ts`

**`scripts/`:**
- Purpose: Build, deploy, and database migration scripts
- Contains: Shell scripts for release workflow, SQL migrations
- Key files: `release.sh` (full pipeline), `init-schema.sql` (base schema)

## Key File Locations

**Entry Points:**
- `crates/server/src/main.rs`: Server binary entry point (791 lines)
- `web/src/main.tsx`: React SPA mount point
- `web/src/app.tsx`: React Router configuration

**Configuration:**
- `crates/server/src/config.rs`: `ServerConfig::from_env()` reads all `MAILRS_*` env vars (1905 lines)
- `Cargo.toml`: Workspace root with shared dependency versions
- `web/vite.config.ts`: Vite build configuration
- `users.toml`: File-based user credentials
- `docker-compose.yml`: Container service definitions

**Core Logic:**
- `crates/server/src/smtp_session.rs`: SMTP connection handling (1274 lines)
- `crates/server/src/imap_session.rs`: IMAP session handling (2480 lines)
- `crates/server/src/inbound/pipeline.rs`: Inbound email decision pipeline (1346 lines)
- `crates/mailbox/src/store.rs`: All mailbox/conversation PG queries (2414 lines)
- `crates/server/src/domain_store.rs`: Domain resolution with caching (1121 lines)
- `crates/server/src/event_bus.rs`: Real-time event system

**Web API:**
- `crates/server/src/web/mod.rs`: Router definition + WebState (1108 lines)
- `crates/server/src/web/mail.rs`: Mail CRUD + send endpoints (1546 lines)
- `crates/server/src/web/conversations.rs`: Conversation endpoints (1261 lines)
- `crates/server/src/web/admin.rs`: Admin management endpoints
- `crates/server/src/web/auth.rs`: Authentication endpoints

**Database:**
- `scripts/init-schema.sql`: Base schema (190 lines, 14 tables)
- `scripts/migrate-*.sql`: Incremental migrations (009 so far)

**Testing:**
- `crates/server/tests/`: Server integration tests
- `web/src/components/__tests__/`: Component tests
- `web/src/store/__tests__/`: Store tests
- `web/src/lib/__tests__/`: Utility tests

## Naming Conventions

**Files (Rust):**
- snake_case: `smtp_session.rs`, `domain_store.rs`, `greylist_db.rs`
- Modules match their primary type: `event_bus.rs` contains `EventBus`

**Files (TypeScript):**
- kebab-case: `use-mail-events.ts`, `conversation-list.tsx`, `error-boundary.tsx`
- Hooks prefixed with `use-`: `use-mail-events.ts`, `use-keyboard-nav.ts`
- Tests in `__tests__/` subdirectories

**Crate Naming:**
- Package names: `mailrs-` prefix (e.g., `mailrs-smtp-proto`, `mailrs-mailbox`)
- Directory names: no prefix (e.g., `crates/smtp-proto/`, `crates/mailbox/`)

**Database:**
- Tables: snake_case plural (`messages`, `mailboxes`, `greylist_triplets`)
- Columns: snake_case (`user_address`, `date_epoch`, `thread_id`)
- Indexes: `idx_` prefix (`idx_messages_thread`, `idx_contacts_user_score`)
- Migrations: `migrate-NNN-description.sql`

## Where to Add New Code

**New SMTP Feature:**
- Protocol changes: `crates/smtp-proto/src/` (pure parsing/formatting)
- Session behavior: `crates/server/src/smtp_session.rs`
- Tests: inline `#[cfg(test)]` modules or `crates/smtp-proto/src/session/tests.rs`

**New Inbound Pipeline Stage:**
- Stage logic: new file in `crates/server/src/inbound/`
- Wire into: `crates/server/src/inbound/pipeline.rs` (add to `PipelineInput` + `make_delivery_decision`)
- Export from: `crates/server/src/inbound/mod.rs`

**New REST API Endpoint:**
- Handler function: appropriate file in `crates/server/src/web/` (mail.rs, admin.rs, conversations.rs, or new file)
- Route registration: `crates/server/src/web/mod.rs` `router()` function
- If new module: add `mod` in `crates/server/src/web/mod.rs`

**New Frontend Page:**
- Page component: `web/src/pages/{page-name}.tsx`
- Route: `web/src/app.tsx` (add `<Route>`)
- State atoms: `web/src/store/{domain}.ts`

**New Frontend Component:**
- Component file: `web/src/components/{component-name}.tsx`
- UI primitives: `web/src/components/ui/{component}.tsx`
- Tests: `web/src/components/__tests__/{component-name}.test.tsx`

**New Shared Utility (Frontend):**
- Utility: `web/src/lib/{util-name}.ts`
- Tests: `web/src/lib/__tests__/{util-name}.test.ts`

**New Custom Hook:**
- Hook: `web/src/hooks/use-{name}.ts`

**New Database Migration:**
- SQL file: `scripts/migrate-NNN-description.sql` (increment from last: currently 009)
- Run manually against PG; no automated migration runner

**New Rust Crate:**
- Directory: `crates/{name}/` (without `mailrs-` prefix)
- Package name in `Cargo.toml`: `mailrs-{name}`
- Register in workspace `Cargo.toml` members array
- Add dependency in consuming crate's `Cargo.toml`

## Special Directories

**`samples/`:**
- Purpose: Sample .eml files for testing/import
- Generated: Imported from IMAP
- Committed: No (gitignored)

**`dist/`:**
- Purpose: Cross-compiled release artifacts
- Generated: Yes, by `scripts/dist.sh`
- Committed: Partially (binary + web assets)

**`web/dist/`:**
- Purpose: Vite production build output
- Generated: Yes, by `bun run build`
- Committed: Yes (deployed as static files)

**`web/node_modules/`:**
- Purpose: Frontend dependencies
- Generated: Yes, by `bun install`
- Committed: No (gitignored)

**`target/`:**
- Purpose: Rust build artifacts
- Generated: Yes, by `cargo build`
- Committed: No (gitignored)

---

*Structure analysis: 2026-03-09*
