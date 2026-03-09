---
phase: 04-mcp-server
plan: 01
subsystem: api
tags: [mcp, rmcp, schemars, axum, rust]

requires:
  - phase: 01-api-key-auth
    provides: api_key_store verify_api_key for MCP auth
  - phase: 02-agent-email-ops
    provides: pub(crate) verify_sender, resolve_thread_reply, deliver_message
provides:
  - MailMcpService with ServerHandler and 5 MCP tools
  - mcp_auth_middleware for Bearer token validation
  - setup_mcp() returning axum Router for /mcp endpoint
affects: [04-02-mcp-wiring]

tech-stack:
  added: [rmcp 1.1, schemars 1.0]
  patterns: [tool_router macro, Parameters<T> wrapper, StreamableHttpService factory]

key-files:
  created:
    - crates/server/src/mcp/mod.rs
    - crates/server/src/mcp/tools.rs
    - crates/server/src/mcp/auth.rs
  modified:
    - crates/server/Cargo.toml
    - crates/server/src/main.rs
    - crates/server/src/web/mod.rs
    - crates/server/src/web/auth.rs
    - crates/server/src/web/mail.rs

key-decisions:
  - "rmcp 1.1 + axum 0.8 confirmed compatible, no dependency conflicts"
  - "Tools defined in #[tool_router] impl block with Parameters<T> pattern (not #[tool(aggr)])"
  - "Auth uses default placeholder AuthUser in factory, real auth deferred to middleware layer"
  - "deliver_message/build_rfc5322_message/resolve_thread_reply changed to pub(crate) for MCP reuse"
  - "AuthUser now derives Clone for MailMcpService compatibility"

patterns-established:
  - "MCP tool params: derive Debug, Deserialize, schemars::JsonSchema"
  - "MCP tools return CallToolResult::success with JSON-stringified Content::text"
  - "MCP errors use ErrorData (aliased as McpError) with invalid_params/internal_error"

requirements-completed: [MCP-01, MCP-03, MCP-04, MCP-05, MCP-06, MCP-07]

duration: 13min
completed: 2026-03-10
---

# Phase 4 Plan 1: MCP Server Core Summary

**rmcp 1.1 MCP service with 5 mail tools (send, read, search, reply, list_conversations) and API key auth middleware**

## Performance

- **Duration:** 13 min
- **Started:** 2026-03-09T18:27:00Z
- **Completed:** 2026-03-09T18:40:00Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- Verified rmcp 1.1 + axum 0.8 compile together (STATE.md blocker resolved)
- Created MailMcpService with ServerHandler impl and all 5 MCP tools
- Auth middleware validates mlrs_ Bearer tokens reusing existing api_key_store logic
- 8 unit tests for tool parameter schema generation and deserialization

## Task Commits

Each task was committed atomically:

1. **Task 1: Add rmcp/schemars dependencies and create MCP module skeleton** - `6412d83` (feat)
2. **Task 2: Implement 5 MCP tools** - `32a31f5` (feat)

## Files Created/Modified
- `crates/server/src/mcp/mod.rs` - MailMcpService struct, ServerHandler, 5 tool implementations, setup_mcp()
- `crates/server/src/mcp/tools.rs` - 5 parameter structs with JsonSchema derives + 8 unit tests
- `crates/server/src/mcp/auth.rs` - mcp_auth_middleware for Bearer token validation
- `crates/server/Cargo.toml` - added rmcp 1.1 + schemars 1.0 dependencies
- `crates/server/src/main.rs` - added mod mcp declaration
- `crates/server/src/web/mod.rs` - re-export AuthMethod, make mail module pub(crate), ApiResult pub(crate)
- `crates/server/src/web/auth.rs` - added Clone derive to AuthUser
- `crates/server/src/web/mail.rs` - changed deliver_message, build_rfc5322_message, resolve_thread_reply, build_rfc5322_with_attachments to pub(crate)

## Decisions Made
- rmcp 1.1 confirmed compatible with axum 0.8 -- no dependency conflicts (resolved STATE.md blocker)
- Used `Parameters<T>` wrapper pattern (not `#[tool(aggr)]`) as per rmcp 1.1 API
- MCP auth uses factory closure with placeholder AuthUser; real auth happens in middleware layer before requests reach MCP service (plan 02 will wire the layer)
- Changed 4 web::mail functions from pub(super) to pub(crate) to enable MCP reuse without code duplication

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed non-exhaustive struct construction for ServerInfo/Implementation**
- **Found during:** Task 1
- **Issue:** rmcp's ServerInfo and Implementation are `#[non_exhaustive]`, cannot use struct literal syntax
- **Fix:** Used builder pattern (ServerInfo::new().with_protocol_version().with_server_info())
- **Files modified:** crates/server/src/mcp/mod.rs
- **Committed in:** 6412d83

**2. [Rule 3 - Blocking] Fixed McpError import path**
- **Found during:** Task 2
- **Issue:** `rmcp::McpError` doesn't exist; the type is `rmcp::ErrorData`
- **Fix:** Used `use rmcp::ErrorData as McpError`
- **Files modified:** crates/server/src/mcp/mod.rs
- **Committed in:** 32a31f5

**3. [Rule 3 - Blocking] Fixed RngCore trait import for OsRng.next_u32()**
- **Found during:** Task 2
- **Issue:** rmcp brings in rand_core 0.10 which requires explicit RngCore trait import
- **Fix:** Added `use rand_core::RngCore;`
- **Files modified:** crates/server/src/mcp/mod.rs
- **Committed in:** 32a31f5

---

**Total deviations:** 3 auto-fixed (3 blocking)
**Impact on plan:** All auto-fixes were rmcp API discovery issues. No scope creep.

## Issues Encountered
None beyond the auto-fixed deviations above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- MCP service ready for wiring into the web router (plan 02)
- setup_mcp() returns an axum Router that can be merged into the main app
- Auth middleware ready to be applied as a layer on the MCP router

---
*Phase: 04-mcp-server*
*Completed: 2026-03-10*
