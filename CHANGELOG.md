# mailrs changelog

Release notes for the mailrs mail server. Format follows [Keep a
Changelog](https://keepachangelog.com/en/1.1.0/). Cycles are dated
using tag-push UTC dates.

Two independent tag streams: `v<major>.<minor>.<patch>` for the Rust
binary + fastcore stack, and `web-v<YYYY.MM.DD>-<seq>` for the React
web UI. Only the Rust stream is enumerated below; web releases are
tracked separately in the release-web workflow.

## Unreleased — accumulating on `develop`, ships as **v2.0.0 GA**

Per user directive 2026-07-06: no interim tags, all remaining Stage
B-H work accumulates on `develop` and ships together as v2.0.0.

- **Stage B.6 (v1.9.4 landed 2026-07-06):** ZINTERSTORE materializes
  multi-filter conversation lists so combined predicates (inbox ∩
  has_unread, starred ∩ archived) return an exact intersection, not
  the highest-priority single index. Per-request temp key with 60 s
  orphan-TTL + post-use del.
- **Stage B.7 / B.8:** kevy 3.17 change feed replaces IMAP IDLE's
  tokio broadcast::channel — durable across restarts, no lost events
  under slow-consumer lag. `Store::changes_since(gen, offset)` with
  500 ms poll cadence. B.7 (idx_create) skipped because our alias /
  domain / account data model uses plain string keys + set indexes,
  not hash-field entries; a data-model migration would be required
  and belongs in a separate RFC.
- **Stage D · G12.5:** `GET /api/admin/audit-log/export?since=&until=
  &actor=&action=` — unrestricted-scan JSON export for bulk retention
  offloading. `AuditQuery` gains `since` / `until` time-window fields
  used by both list + export handlers. Existing 50 K row count-cap
  retention retained — more robust than time-window sweeps under
  bursty load.
- **Stage D · G13.3:** `POST /api/scheduled/{id}/cancel` +
  `/reschedule` — outbound queue control on the SCHEDULED zset.
  Sender-verified; reschedule enforces future timestamp; sender
  mismatch returns 404 to prevent id enumeration.
- **Stage C.5 (partial):** `mcp.rs` → `mcp/{mod,params}.rs`, 21
  Params structs extracted. mod.rs shrunk 1228 → 999 lines. Further
  per-category tool splits + 25 additional MCP tools (groups /
  permissions / email-groups / apps / encryption / system-config /
  greylist / queue / audit / reconcile / list_aliases) tracked for
  Stage C.1-C.4 within this cycle.
- **Upstream tracking:** kevy-client 1.13 does not wrap kevy-server's
  3.17 features (brpop / hexpire / zinterstore / idx / changes_since).
  Phase 4 (BRPOP), Phase 5 (HEXPIRE), and Phase 7/8 for network paths
  remain blocked. See `.claude-profile-2/.../memory/feedback-kevy-client-1.13-gaps.md`.
- **Docs / rules:** ARCHITECTURE.md rewrite, PERFORMANCE.md v2 row,
  DEPS_AUDIT.md updates, `.claude/rules/kevy-patterns.md`, DEPLOY.md
  runbook — pending, will land within the v2.0.0 window.

## v1.9.4 — 2026-07-06

- Stage B.6 · ZINTERSTORE — see Unreleased.

## v1.9.3 — 2026-07-06

- Stage B.3 · N+1 read fanout collapsed into atomic snapshot closures.
  `list_threads_by_activity` / `list_thread_messages` on mailbox-kevy;
  `search.rs` linear-fallback consolidated from up to 500 kevy_client
  Connections down to one.

## v1.9.2 — 2026-07-06

- Stage B.2 · Atomic counters. `allocate_uid` + `register_uid` +
  `uidvalidity` collapse read-check + INCR + rev/forward index write
  into a single AtomicCtx closure. 100-thread same-mid idempotency
  regression test added. Three duplicate `next_id` helpers
  (complete/prefs/admin) replaced by a bare `c.incr()`.

## v1.9.1 — 2026-07-06

- Stage B.1 · 10 mailbox-kevy CRUD methods + `ingest_delivered_file`
  self-heal + server session table + auth 2FA recovery-code all
  collapse multi-op RMW into single `store.atomic(|ctx| ...)`
  closures. kevy 3.17 `AtomicCtx` gained `zrem` / `hdel` / `del` /
  `sadd` / `srem`, retiring the 1.15-era two-step workarounds.

## v1.9.0 — 2026-07-06

- **Foundation for v2.0.0:** kevy-embedded 1.15 → 3.17.2 + kevy-client
  1.12 → 1.13.1 workspace lift. Zero source-level breaking changes —
  core op signatures identical between 1.15 and 3.17.
- 1.x AOF forward-compat proven: prod 531 MB / 3.7 M-command AOF
  replays clean on the 3.17 binary in 1.68 s (dbsize=84708, 40
  aliases intact).
- Compose consolidation: root `docker-compose.{prod,prod.split,}.yml`
  deleted (legacy monolith duplicates); canonical files live under
  `deploy/`.
- kevy container pinned `latest` → `3.17.1` in prod / staging / split
  composes.
- CI: `STAGING_GATE_GRACE` cleared to `__none__` sentinel so v2 tags
  never bypass the staging soak gate. `release-web.yml` dead
  `up -d mailrs` step (targeted the pre-fastcore monolith service)
  removed. `scripts/staging-fastcore-up.sh` parametrized IMAGE_TAG
  via `MAILRS_IMAGE_TAG` env.

## v1.8.11 — 2026-07-06

- MCP tool port batch, alias case-sensitivity fix, misc fmt.

## v1.8.5 – v1.8.10 — 2026-07-05 / 06

- Alias recovery lineage: case-sensitivity in `resolve_alias`
  (byte-eq cycle detect), AliasStore trait abstraction, network kevy
  backend flip, mobile-mail conversation-panel bleed fix.

## v1.8.0 – v1.8.4 — 2026-06 / 07

- fastcore 4-process split arrives in prod. receiver + fastcore +
  webapi-fc + fastcore-sender + shared kevy container. `SPG` lane
  retained on staging as the pg-core dogfood dual-mode partner.

## v1.7.x — 2026-05 / 06 / 07

- Original monolith + spg-dogfood iterations. Notable rollouts:
  v1.7.95 kevy embedded cutover, v1.7.132 web bind-mount rollout,
  v1.7.148 SPG cutover, v1.7.170 prod livelock hotfix, v1.7.180
  final baseline before v1.8.

## Earlier

- v1.6 and earlier are covered by GitHub Releases only.
