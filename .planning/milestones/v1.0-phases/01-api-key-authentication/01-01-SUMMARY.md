---
phase: 01-api-key-authentication
plan: 01
subsystem: auth
tags: [api-key, authuser, sha256, valkey, sqlx, postgres]

# Dependency graph
requires: []
provides:
  - AuthUser named-field struct with address, display_name, super_domains, auth_method
  - api_key_store module with key generation, DB CRUD, Valkey cache helpers
  - api_keys migration DDL
affects: [01-02-PLAN]

# Tech tracking
tech-stack:
  added: [chrono serde feature]
  patterns: [AuthUser struct destructuring across handlers, validate_domains accepts slice instead of state]

key-files:
  created:
    - crates/server/src/api_key_store.rs
    - scripts/migrate-010-api-keys.sql
  modified:
    - crates/server/src/web/auth.rs
    - crates/server/src/web/mod.rs
    - crates/server/src/web/admin.rs
    - crates/server/src/web/mail.rs
    - crates/server/src/web/conversations.rs
    - crates/server/src/web/templates.rs
    - crates/server/src/web/ai_assist.rs
    - crates/server/src/main.rs
    - crates/server/Cargo.toml

key-decisions:
  - "validate_domains takes &[String] slice instead of &WebState, decoupling domain validation from session storage"
  - "auth_me reads directly from AuthUser fields, removing State dependency"

patterns-established:
  - "AuthUser destructuring: use `AuthUser { address: user, .. }` for handlers needing address, `AuthUser { address: ref user, ref super_domains, .. }` for handlers calling validate_domains, `AuthUser { .. }` for admin handlers"

requirements-completed: [AKEY-02, AKEY-03, AKEY-06, AKEY-07]

# Metrics
duration: 8min
completed: 2026-03-10
---

# Phase 1 Plan 01: AuthUser Refactor + API Key Store Summary

**AuthUser refactored to named-field struct with AuthMethod enum; api_key_store module providing mlrs_ key generation, sqlx CRUD, and Valkey cache helpers**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-09T16:53:42Z
- **Completed:** 2026-03-09T17:01:27Z
- **Tasks:** 2
- **Files modified:** 11

## Accomplishments
- AuthUser converted from tuple struct to named-field struct carrying address, display_name, super_domains, auth_method
- validate_domains decoupled from sessions DashMap, now accepts &[String] directly
- api_key_store module with generate_api_key (mlrs_ format), insert/get/list/revoke DB operations, and Valkey cache helpers
- Migration script for api_keys table with partial indexes on active keys

## Task Commits

Each task was committed atomically:

1. **Task 1: Refactor AuthUser to named-field struct** - `c0055af` (refactor)
2. **Task 2: Create api_keys migration and api_key_store module** - `bd0dcd0` (feat)

## Files Created/Modified
- `crates/server/src/web/auth.rs` - AuthUser struct + AuthMethod enum + updated from_request_parts and auth_me
- `crates/server/src/web/mod.rs` - validate_domains now takes &[String], tests updated
- `crates/server/src/web/admin.rs` - AuthUser destructuring updated (14 handlers)
- `crates/server/src/web/mail.rs` - AuthUser destructuring updated (15 handlers)
- `crates/server/src/web/conversations.rs` - AuthUser destructuring with super_domains for validate_domains callers
- `crates/server/src/web/templates.rs` - AuthUser destructuring updated
- `crates/server/src/web/ai_assist.rs` - AuthUser destructuring updated
- `crates/server/src/api_key_store.rs` - new module with key generation, DB CRUD, Valkey cache
- `scripts/migrate-010-api-keys.sql` - api_keys table DDL with indexes
- `crates/server/src/main.rs` - added api_key_store module declaration
- `crates/server/Cargo.toml` - enabled chrono serde feature

## Decisions Made
- validate_domains takes `&[String]` slice instead of `&WebState`, decoupling domain validation from session storage
- auth_me reads directly from AuthUser fields, removing State dependency entirely
- chrono serde feature enabled in server crate (needed for CachedApiKey serialization)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Enabled chrono serde feature**
- **Found during:** Task 2 (api_key_store module)
- **Issue:** CachedApiKey with `DateTime<Utc>` fields needed Serialize/Deserialize, but chrono's serde feature was not enabled
- **Fix:** Added `features = ["serde"]` to chrono dependency in crates/server/Cargo.toml
- **Files modified:** crates/server/Cargo.toml
- **Verification:** cargo build succeeds
- **Committed in:** bd0dcd0 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Essential for compilation. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- AuthUser struct ready to carry API key context (AuthMethod::ApiKey variant)
- api_key_store functions ready to be wired into auth middleware (Plan 02)
- Migration script ready to apply to database
- All 1049 existing tests pass

---
*Phase: 01-api-key-authentication*
*Completed: 2026-03-10*
