---
phase: 01-api-key-authentication
verified: 2026-03-10T12:00:00Z
status: passed
score: 11/11 must-haves verified
---

# Phase 1: API Key Authentication Verification Report

**Phase Goal:** Agent 能通过 API key 认证访问 mailrs API，权限继承账号角色
**Verified:** 2026-03-10
**Status:** passed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | AuthUser carries address, display_name, super_domains, auth_method as named fields | VERIFIED | `auth.rs:33-38` — pub struct with all four named fields |
| 2 | validate_domains reads super_domains from AuthUser, not from sessions DashMap | VERIFIED | `mod.rs:264-267` — signature is `(Option<&str>, &[String])`, no WebState param |
| 3 | auth_me reads display_name and super_domains from AuthUser, not from sessions DashMap | VERIFIED | `auth.rs:318-326` — destructures AuthUser directly, no State param |
| 4 | api_keys table exists with prefix, key_hash, account_address, expires_at, revoked_at columns | VERIFIED | `migrate-010-api-keys.sql:1-10` — CREATE TABLE with all columns |
| 5 | api_key_store module can generate, lookup, and revoke API keys | VERIFIED | `api_key_store.rs` — generate_api_key (L41), get_api_key_by_prefix (L81), revoke_api_key (L114) |
| 6 | Agent can authenticate with Authorization: Bearer mlrs_... and get AuthUser with correct address and super_domains | VERIFIED | `auth.rs:68-71` — mlrs_ prefix branch in from_request_parts; `auth.rs:92-194` — full verify_api_key with hash check, expiry, cache, super_domains resolution |
| 7 | User can create API key via POST /api/agent/keys (session auth required), receives full key once | VERIFIED | `api_key.rs:43-119` — create handler with session-only guard (L49), returns full key with warning (L91-98) |
| 8 | User can list their API keys via GET /api/agent/keys (metadata only, no key_hash) | VERIFIED | `api_key.rs:122-156` — list handler maps to ApiKeyResponse (no key_hash field, L33-40) |
| 9 | User can revoke API key via DELETE /api/agent/keys/{id}, key immediately stops working | VERIFIED | `api_key.rs:159-194` — revoke handler with UPDATE RETURNING prefix |
| 10 | Revoked key's Valkey cache is evicted on revoke | VERIFIED | `api_key.rs:177-178` — cache_delete called with returned prefix |
| 11 | Expired key returns 401 | VERIFIED | `auth.rs:163-166` — expires_at check returns "api key expired"; unit test `test_expired_key_rejected` passes |

**Score:** 11/11 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/server/src/web/auth.rs` | AuthUser named-field struct with AuthMethod enum + verify_api_key | VERIFIED | 327 lines, struct at L33, verify_api_key at L92, mlrs_ branch at L69 |
| `crates/server/src/api_key_store.rs` | API key generation, DB/cache CRUD | VERIFIED | 243 lines, generate/insert/get/list/revoke + Valkey cache helpers + 3 unit tests |
| `scripts/migrate-010-api-keys.sql` | api_keys table DDL | VERIFIED | 14 lines, CREATE TABLE + 2 partial indexes |
| `crates/server/src/web/api_key.rs` | CRUD handlers for API key management | VERIFIED | 358 lines, create/list/revoke handlers + 8 unit tests |
| `crates/server/src/web/mod.rs` | Route registration for /api/agent/keys | VERIFIED | L19 `mod api_key;`, L665-666 route registration |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `web/mod.rs` | `web/auth.rs` | validate_domains uses AuthUser.super_domains | WIRED | `validate_domains(_, &[String])` signature; callers pass `&auth_user.super_domains` |
| `api_key_store.rs` | `migrate-010-api-keys.sql` | sqlx queries against api_keys table | WIRED | INSERT/SELECT/UPDATE queries reference `api_keys` table matching DDL schema |
| `web/auth.rs` | `api_key_store.rs` | verify_api_key calls cache_get/cache_set/get_api_key_by_prefix | WIRED | L105 cache_get, L119 get_api_key_by_prefix, L149 cache_set, L157 sha256_hex, L174 update_last_used |
| `web/api_key.rs` | `api_key_store.rs` | CRUD handlers call insert/list/revoke/cache_delete | WIRED | L76 generate_api_key, L78 insert_api_key, L136 list_api_keys, L174 revoke_api_key, L178 cache_delete |
| `web/mod.rs` | `web/api_key.rs` | route registration for /api/agent/keys | WIRED | L665-666 routes registered with post/get/delete |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| AKEY-01 | 01-02 | User can create API key for their account | SATISFIED | `api_key.rs:43` POST handler creates key |
| AKEY-02 | 01-01 | API key shown once on creation, stored as SHA-256 hash | SATISFIED | `api_key.rs:91-98` returns full key once; `api_key_store.rs:50` sha256_hex for storage |
| AKEY-03 | 01-01 | API key uses mlrs_ prefix, first 8 chars stored as plaintext identifier | SATISFIED | `api_key_store.rs:49` format `mlrs_{prefix}_{secret}`; DB stores prefix column |
| AKEY-04 | 01-02 | API key authenticates via Authorization: Bearer | SATISFIED | `auth.rs:54,69-71` Bearer extraction + mlrs_ branch |
| AKEY-05 | 01-02 | User can revoke API key with immediate effect (including Valkey cache eviction) | SATISFIED | `api_key.rs:159-194` revoke + cache_delete |
| AKEY-06 | 01-01 | API key inherits account role; superadmin key can operate any mailbox | SATISFIED | `auth.rs:125-137` resolves super_domains from domain_store; test `test_inherits_account_role` confirms |
| AKEY-07 | 01-01 | API key supports optional expiration time | SATISFIED | `api_key_store.rs:62` expires_at param; `auth.rs:163-166` expiry check |

No orphaned requirements found -- all 7 AKEY requirements are mapped to plans and satisfied.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none) | - | - | - | No anti-patterns detected |

### Human Verification Required

### 1. End-to-End API Key Flow

**Test:** Create an API key via POST /api/agent/keys with session auth, then use the returned key as `Authorization: Bearer mlrs_...` on any existing endpoint (e.g., GET /api/conversations).
**Expected:** Authentication succeeds, response contains data scoped to the key's account.
**Why human:** Requires running server with PG + Valkey, cannot verify full network auth flow statically.

### 2. Superadmin Key Cross-Mailbox Access

**Test:** Create an API key for a superadmin account (one with super_domains set), use it to access another user's mailbox endpoints with `?domains=` parameter.
**Expected:** Returns data from the other domain's mailbox.
**Why human:** Requires actual domain_store data and multi-account setup.

### 3. Revoke Immediate Effect

**Test:** Create key, verify it works, revoke it via DELETE, immediately retry with the same key.
**Expected:** First request succeeds, POST revoke returns 200, subsequent request returns 401.
**Why human:** Requires live server with Valkey cache to verify cache eviction timing.

### Gaps Summary

No gaps found. All 11 observable truths verified, all 5 artifacts substantive and wired, all 5 key links confirmed, all 7 requirements satisfied. 11 unit tests pass (8 in api_key.rs + 3 in api_key_store.rs). Code is clean with no TODOs, FIXMEs, or placeholder implementations.

---

_Verified: 2026-03-10_
_Verifier: Claude (gsd-verifier)_
