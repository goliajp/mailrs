---
phase: 01-api-key-authentication
plan: 02
subsystem: auth
tags: [api-key, bearer-auth, axum-extractor, valkey-cache, crud]

# Dependency graph
requires:
  - phase: 01-01
    provides: AuthUser struct with AuthMethod enum, api_key_store module
provides:
  - verify_api_key branch in AuthUser extractor (mlrs_ prefix detection)
  - CRUD endpoints for API key management (create/list/revoke)
  - Valkey cache eviction on key revocation
  - Unit tests for key format, hash, auth logic, expiration, inheritance
affects: [02-agent-mail-api, 04-mcp-server]

# Tech tracking
tech-stack:
  added: []
  patterns: [verify_api_key_logic helper for testable auth without DB, session-only guard on create]

key-files:
  created:
    - crates/server/src/web/api_key.rs
  modified:
    - crates/server/src/web/auth.rs
    - crates/server/src/web/mod.rs
    - crates/server/src/api_key_store.rs

key-decisions:
  - "revoke_api_key returns Option<String> (prefix) instead of bool, enabling cache eviction without extra query"
  - "API keys cannot create other API keys (session-only guard on POST /api/agent/keys)"
  - "verify_api_key_logic extracted as testable pure function accepting token + CachedApiKey"

patterns-established:
  - "API key CRUD at /api/agent/keys with session-only create, any-auth list, any-auth revoke"
  - "Valkey cache eviction on revoke via returned prefix from UPDATE RETURNING"

requirements-completed: [AKEY-01, AKEY-04, AKEY-05]

# Metrics
duration: 4min
completed: 2026-03-10
---

# Phase 1 Plan 02: API Key Auth Extractor + CRUD Endpoints Summary

**Bearer mlrs_ token auth in extractor with Valkey-cached verification, plus create/list/revoke endpoints at /api/agent/keys**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-09T17:05:04Z
- **Completed:** 2026-03-09T17:08:51Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- verify_api_key wired into AuthUser::from_request_parts: mlrs_ tokens now authenticate via cache/DB lookup with hash verification and expiry check
- CRUD handlers for /api/agent/keys: create (session-only, returns full key once), list (metadata only), revoke (with Valkey cache eviction)
- 8 unit tests covering key format, hash, valid auth, expired key, wrong secret, malformed key, superadmin inheritance

## Task Commits

Each task was committed atomically:

1. **Task 1: Add verify_api_key to auth extractor and create CRUD handlers** - `2da99a1` (feat)

Tests were included in the same commit since they are pure unit tests co-located in api_key.rs.

## Files Created/Modified
- `crates/server/src/web/api_key.rs` - CRUD handlers + 8 unit tests for API key management
- `crates/server/src/web/auth.rs` - verify_api_key function + mlrs_ branch in from_request_parts + Debug derive
- `crates/server/src/web/mod.rs` - api_key module declaration + route registration at /api/agent/keys
- `crates/server/src/api_key_store.rs` - sha256_hex made pub(crate), revoke_api_key returns Option<String>

## Decisions Made
- revoke_api_key changed to return prefix via UPDATE RETURNING instead of bool, avoiding extra SELECT for cache eviction
- API keys cannot create other API keys — session-only guard prevents key escalation
- Tests use verify_api_key_logic helper that mirrors actual verify_api_key but accepts CachedApiKey directly, enabling pure unit tests without DB/Valkey

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Added Debug derive to AuthUser**
- **Found during:** Task 1 (compilation)
- **Issue:** Tests using unwrap_err() required Debug on AuthUser
- **Fix:** Added `#[derive(Debug)]` to AuthUser struct
- **Files modified:** crates/server/src/web/auth.rs
- **Committed in:** 2da99a1

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Trivial derive addition. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- API key authentication fully operational for all existing endpoints
- AI agents can authenticate with `Authorization: Bearer mlrs_...` tokens
- Phase 2 (Agent Mail API) can build on this auth infrastructure
- Migration script from Plan 01 must be applied to database before use

---
*Phase: 01-api-key-authentication*
*Completed: 2026-03-10*

## Self-Check: PASSED
