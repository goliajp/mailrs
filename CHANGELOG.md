# mailrs changelog

Release notes for the mailrs mail server. Format follows [Keep a
Changelog](https://keepachangelog.com/en/1.1.0/). Cycles are dated
using tag-push UTC dates.

Two independent tag streams: `v<major>.<minor>.<patch>` for the Rust
binary + fastcore stack, and `web-v<YYYY.MM.DD>-<seq>` for the React
web UI. Only the Rust stream is enumerated below; web releases are
tracked separately in the release-web workflow.

## Since v2.0.0 — shipped straight from `develop`, up to **v2.87.0**

Per-version enumeration stopped at `v2.0.0` (2026-07-07). Everything
since has shipped by `scripts/direct-deploy.sh <version>`, which takes
the version on the command line, and this section kept accumulating
under a heading that still said "ships as v2.2.0" while production ran
2.85.0. The notes below are per change, newest first, with the version
it landed in where that is known.

### `Hao` — a subject cut from the reader's own address (2.87.0)

A cold-sales campaign sends mail whose subject is one word: a piece of
the recipient's mailbox read as a first name — `Hao` to
`lihao@golia.jp` — with a body opening `Hao, would 4 to 8 more
high-paying contracts … move the needle?`, from a domain registered for
the purpose. 21 of 39,685 production messages have that subject shape
and all 21 are this campaign. The new rule `subject-cut-from-the-address`
scores 5 — the Junk threshold — and never holds: it is cold sales, not
fraud on its face.

A scored rule used to reach only new mail. The historical sweep acted on
hold-grade findings and nothing else, so everything delivered before a
scored rule existed stayed where it was. It now also moves a conversation
nothing holds, whose fraud score alone reaches the threshold, to Junk —
the sum the receive path already uses — and the backfill journal counts
them as `junked`. Run first on a local copy of production: 37,018
conversations, 29 scored into Junk (16 of this campaign, 13 Amazon /
Apple / JCB phishing the brand rules score at 7.5–9), 18 moved and 11
already there; no hold added or released.

### A sender with no avatar at all

`Microsoft Rewards` from `customeremail.microsoftrewards.com` drew no
avatar — not the letter fallback, nothing. The icon cascade asks
Google (404 for that domain) and then DuckDuckGo, which answers `200
image/x-icon` with **43 bytes**: a 1×1 transparent GIF, its way of
saying it has no icon. Any 200 counted as a hit, so the UI stretched
one transparent pixel into a 36 px circle.

The cascade now measures what it got — PNG, GIF, ICO and JPEG state
their size in the header, SVG (BIMI) is taken as-is — and treats
anything under 8×8 as the provider saying no, carrying on to the next
source and finally to the letter avatar. Hits live in kevy for a week,
so the same test runs on the way out of the cache: placeholders
already stored are dropped on first touch instead of lingering for
seven days.

### The sweep can tell how a stored message was submitted

`claims-our-domain` needs to know that a message arrived on a session
that did not authenticate, and the historical sweep had no way to know
— so it declined, and mail already delivered could never be re-judged
by it.

It can now read it off the message: the receiver stamps
`Authentication-Results:` on the inbound path only, which runs only
for unauthenticated sessions, and prepends it. A stored message whose
**first** such header names this deployment came in as a stranger; one
our own people submitted carries none of ours. That needs
`MAILRS_HOSTNAME` on the fastcore process, which only the receiver had
— now in its compose block. Unset, the sweep says so in its log and
the rule keeps declining, the same shape as the missing org names.

### Mail claiming our own domain is held for review

A BEC message from `aiyhccspbu@golia.jp` — display name `齋藤 真`,
subject `ギリア株式会社 業務変更`, asking the reader to reply with
their personal LINE QR code — reached the inbox with a "Suspicious
sender" badge and nothing else. Every hold-worthy identity rule starts
at `external(m)`, which is false for our own domains: the exemption
written for outsiders claiming our *name* also excused outsiders using
our *domain*. `minted-address` could not fire either, because `golia`
is a word somebody chose.

New rule `claims-our-domain` (identity, 6.0, holds). The fraud scan
runs only on sessions that did not authenticate — our own people submit
authenticated — so a `From:` at one of `MAILRS_LOCAL_DOMAINS` there is
somebody outside saying they are us, which is also what
`_dmarc.golia.jp` (`p=quarantine; sp=reject`) already says to do with
it. Domains on the org-name allow-list stay exempt.

It needs a new fact, `unauthenticated`, which the receiver sets and the
historical sweep leaves false — so a rescan, which cannot know how a
stored message was submitted, can never hold the mailbox's own internal
history. That also means this catches new mail only; nothing already
delivered moves.

Not covered: the auth facts (`spf` / `dkim` / `dmarc`) are exposed to
the rules but still never filled, because the fraud scan runs before
the pipeline stage that computes them. A rule keyed on authentication
is not expressible until that order changes.

#### Corrected in v2.86.0 — it held 62 of this deployment's own mail

The paragraph above is wrong in the way that matters: "our own people
submit authenticated" is not true of this deployment's own services.
They submit to the MX without SMTP AUTH, from the container bridge, and
some of them are unsigned — `devops@` (38 conversations), `alias-verify@`
(36), `noreply@`, `qa@`, `postmaster@`, all `spf=softfail dkim=none
dmarc=fail`. Of 108 messages in that class, three were the real thing.
The sweep did hold them too: by then it read our own
`Authentication-Results` stamp back off the stored message, so
"unauthenticated" was answerable historically after all, and the
activation's backfill applied it to 36,676 threads.

Rolled back to the previous bundle first (the backfill released all 63),
then the rule was given the two facts that can tell our own mail from a
forgery:

* **where the connection came from.** Our services reach us at
  172.18.0.1; a forger cannot borrow our own host's address. The
  receiver reads it off the socket, the sweep off the first `Received:`
  line it wrote, and a message with no line to read counts as outside.
* **what alignment said.** `dmarc == 'fail'`, not `~= 'pass'`:
  production has mail from a public address that IS us and proves it
  (`spf=pass dmarc=pass`), and a rule firing on "not proven" would hold
  that too. Unknown declines.

The receiver could not answer the second one at all — it scans before
the pipeline stage that checks alignment — so it now scans again
afterwards, and that pass is the verdict `ingest.rs` holds a
conversation by. The accept/junk/reject decision above it is made
before, and is not revisited.

The rule's tests are three real production shapes copied out of the
maildir: the reported BEC is held, the control plane's own mail is not,
and neither is the verified public sender at our domain.

Rust-side kevy 3.17 network-op adoption + admin-panel data-source
repair. Sits on top of the shipped `v2.0.0` GA and `web-v2026.07.08-1`
web release; v2.1 web work is complete and covered further down.

### kevy 3.17 network path (Phase 1-3)

kevy team shipped `kevy-client 1.14.0` on 2026-07-08 with the wraps
that were gating every network-side kevy 3.x adoption path —
BRPOP / BLPOP / BZPOPMIN / HEXPIRE / HPEXPIRE / HPERSIST /
ZINTERSTORE-with-weights / IDX_* / FEED_READ / pipeline. Workspace
lifted 1.13 → 1.14 (API-compatible, no code churn).

- **`mailrs-fastcore-sender`** — main loop was
  `rpop(PENDING_KEY, 1) + sleep(cfg.poll_ms.max(500))` polling.
  Migrated to `brpop(&[PENDING_KEY], Some(5s))` on a
  `spawn_blocking` thread. Queue-arrival wake-up: `~poll_ms/2` avg
  → **0 ms**; idle CPU: 2 kevy RTTs per `poll_ms` → **~zero**;
  wake-up floor unchanged on a hot queue. Commit `4f073eb6`.
- **`mailrs-fastcore::bounce::spawn_bounce_drain`** — outer
  `sleep(10 s)` between `drain_once` invocations replaced by an
  in-drain `brpop(&[BOUNCE_PENDING], Some(10 s))` on the first
  pop, followed by non-blocking `rpop` for burst throughput.
  Bounce delivery latency: uniform 0-10 s → **0 ms** on hot,
  timer-bounded on idle. Commit `15f953cc`.

Phases 4-8 status after this cycle's audit:

- Phase 4 · HEXPIRE for sidecar TTL — **N/A**. Every `HSET +
  EXPIRE` pattern in webapi expires the whole hash key, which is
  normal Redis usage, not the sidecar-key antipattern kevy team
  warned about. No `<key>:expires_at` sidecar keys grep in the
  codebase.
- Phase 5 · ZINTERSTORE for webapi conversation-list filter —
  **already done**. webapi's list handler delegates to fastcore
  RPC; fastcore's `list_threads_by_activity` in
  `mailbox-kevy/src/list_threads.rs` already uses `zinterstore`
  (landed v1.9.4 Stage B.6).
- Phase 6 · idx_query for admin CRUD — **deferred**, needs a
  data-model migration to hash-field entries.
- Phase 7 · feed_read for WS bridge — **deferred**, no
  user-visible regression from the current pubsub broadcast.
  WS clients refetch full state on reconnect, so lost pubsub
  events between webapi restarts self-recover.
- Phase 8 · core-sidestate 4-shard `atomic()` risk — **blocked**
  on the kevy team's internal `docs/KEVY_ADOPTION_GUIDE.md`
  (`atomic_all_shards` pattern; note flagged this in the
  2026-07-08 response).

### Admin data-source repair

Session-observed regressions from the fastcore-split /
network-kevy-alias flip: several webapi admin handlers still
walked legacy pre-split keys that were emptied at cutover, so the
admin UI rendered empty state for a super-admin caller.

- `handlers::admin::list_aliases` / `add_alias` / `remove_alias`
  → route through fastcore `state.core.list_local_aliases` /
  `upsert_local_alias` / `delete_local_alias`. `id` field is a
  deterministic i64 hash of `source_address` (JS safe-integer
  bounded) so the frontend's delete-by-id round-trip works
  without a schema break. Commit `a23a2cf3`.
- `handlers::complete::audit_accounts` → route through
  `state.core.list_accounts` (fastcore embedded kevy). Populates
  the `AuditAccount` shape from `AccountWire`. Commit `a23a2cf3`.
- `handlers::admin::list_domains` / `add_domain` / `remove_domain`
  → route through fastcore `state.core.list_domains` /
  `add_domain` / `remove_domain`. Unblocks the alias-create form's
  domain dropdown (it was a required field, so empty dropdown
  meant no aliases could be added through the UI). Commit
  `b1fa90f5`.
- **Domain-index self-heal.** `fastcore::add_account_route`
  auto-`upsert_domain(a.domain)`; webapi `handlers::admin::add_alias`
  auto-`state.core.add_domain(domain_from_source)`. Prior gap:
  neither path touched the domain index, so a fresh domain was
  invisible in the UI until manual `POST /admin/domains`. Failure
  swallowed with warn — the account/alias write is authoritative,
  the domain index is a UI derived view. Commit `2994f700`.

### Web-only fixes bundled into this cycle

Session-observed:

- Dashboard "Unread" badge was reading
  `data.stats.unread_messages` (`/mail/stats`) with a fallback to
  `data.folders.INBOX.unseen` (`/mail/folders`) — both server-side
  aggregates that don't invalidate on mark-read. Optimistic
  `patchAllInfiniteLists` only touches `conversationKeys.list()`,
  so the badge stayed stuck at the pre-mutation count for the 60 s
  `REFRESH_INTERVAL`. Derive from `data.conversations` instead
  (same cache line the mutation patches). Semantic is now
  thread-count-of-unread, matching `useCurrentUnreadCount`. Commit
  `a69420b6`.

## Unreleased-web — accumulating on `develop`, ships as **web-v2.1.0**

Web-side architectural redesign begun 2026-07-07. The Rust
binary + fastcore stack landed as `v2.0.0` on 2026-07-07; this
section covers the web-only redesign that ships next.

### Web v2.1 — Phase 7-10 (Zod wire boundary complete)

Phase 10 closes the wire migration:

- **Zod at every wire boundary.** Every network call now
  Zod-parses its response — no `as any`, no raw JSON out of
  `fetch`. Coverage: 0 → ~165 Zod-guarded `wireFetch` callsites.
  100 % of the mail hot path, 100 % of auth, 100 % of user
  settings CRUD, 100 % of admin CRUD (12 resources × ~4 endpoints
  each) all routed through `wire/endpoints/*`.
- **Uniform admin CRUD.** `wire/endpoints/admin.ts` provides
  `adminListGet<T>` / `adminObjectGet<T>` / `adminPost<T>` /
  `adminPut<T>` / `adminPatch<T>` / `adminDelete` — Zod-parses
  the envelope (list-vs-object shape) but keeps content
  permissive so downstream keeps its cast contract. 101 admin
  callsites bulk-migrated.
- **Multipart send + inline upload.** `wireFetch` gained
  `bodyRaw?: BodyInit` to carry `FormData` through the same
  Zod-validated response path. `wireSendMailMultipart` +
  `wireUploadInlineImage` replace two raw `fetch('/api/...')`
  callsites in `send-mail.ts` and `rich-editor.tsx`.
- **ESLint boundary gate.** `no-restricted-imports` blocks new
  callers from `postJson` / `putJson` / `deleteJson` /
  `fetchJson` / `fetchList` from `@/lib/api` outside of
  `src/wire/**` and the shim itself. New raw-fetch = build
  failure.
- **`lib/api.ts` collapsed to a domain-adapter shim.** The
  remaining exports (`snoozeConversation`,
  `unsnoozeConversation`, `recordFeedback`, `saveDraft`,
  `deleteDraft`, `toggleReaction`, `getThreadReactions`,
  `listDrafts`) delegate to wire adapters via lazy import.
  Raw `postJson` / etc. helpers survive only because the
  shim + its co-located tests reference them.

Real bugs surfaced by Zod alignment during Phase 10:

1. **`TotpSetup.qr_url` was fabricated** — backend actually
   returns `otpauth_url` (`crates/webapi/src/handlers/complete.rs::
   totp_setup` line 336). The frontend type in `_shared.tsx`
   invented `qr_url`, so the QR display paragraph was silently
   rendering `undefined` for the entire life of the TOTP-setup
   panel. Type + render site both switched to `otpauth_url`.
2. **`OidcClientConfig` shape mismatched backend.** Frontend
   expected `{enabled, login_url, provider_name}` but backend
   sends `{enabled, providers[]}`. Schema now accepts both;
   UI reconciliation deferred (OIDC login isn't the primary
   sign-in path).
3. **`change_password` field name.** Frontend was passing
   `old_password`; backend expects `current_password`
   (`ChangePasswordRequest` line 246). Adapter aligned.
4. **`ReactionSummary.reacted` was fabricated in test fixtures.**
   Wire truth is `me: boolean` (`crates/webapi/src/handlers/mail.rs::
   get_thread_reactions` line 163). Test fixtures updated; adapter
   aligned with `types.ts::ReactionSummary`.
5. **`render-preview` returned a different shape** than the
   frontend expected — backend actually emits `{png_base64,
   fallback_html?}`, frontend fabricated `{previews[],
   errors[]}`. Panel was silently broken. Schema is permissive
   (`passthrough`) for now; UI reconciliation queued.

Phase 7 (design-token discipline):

- **Motion tokens** (`--duration-fast/base/slow`,
  `--ease-standard/emphasized`, `--animate-slide-up`) —
  bottom-sheet + context-menu now use `animate-slide-up`
  instead of inline `animate-[slideUp_200ms_ease-out]`.
- **Elevation tokens** (`--shadow-elevation-0..3` with dark-mode
  overrides). Adoption is incremental — legacy
  `shadow-{sm,md,lg,xl,2xl}` callsites (14 spots) stay until
  touched.

Phase 8 (route data-router):

- **Route-level error boundary.** `RouteErrorFallback` renders
  under every route branch (`errorElement`) — a thrown
  `WireErrorException` (auth / server / validation / network),
  render crash, or 404 shows a specific message with a targeted
  action (sign-in for auth-expired, reload for network, home
  for anything else), replacing the browser's grey unstyled
  reload prompt.

Deferred to a follow-up (design decision needed):

- Route loaders for `/mail` + `/` that call
  `queryClient.ensureQueryData()` — needs a decision on
  whether the loader reads initial filter state from URL params
  or from a jotai store snapshot.
- Full `shadow-{sm,md,lg,xl,2xl}` → `shadow-elevation-*`
  migration.
- Skeleton CLS audit — requires DevTools Performance Insights
  runs, not scriptable.
- Cross-screen mutation matrix regression tests (dashboard ↔
  mail-list ↔ thread-view × mark-read / star / archive / etc.)
  — the mutation-callsite convergence in Phase 10 makes this
  primarily a manual QA pass rather than an automated matrix.

### Web v2.1 — Phase 1-6 shipped 2026-07-07 → 08

- **RQ single-source-of-truth.** The mail list, dashboard, and thread
  view now share one cache line
  (`conversationKeys.infinite(filter)` +
  `conversationKeys.list(filter)`). Any mutation invalidates the
  entire prefix, so a mark-read on `/mail` immediately reflects in
  the home-page unread badge — the original user-reported bug that
  triggered the redesign.
- **Deleted atoms.** `conversationsAtom`, `threadMessagesAtom`,
  `unreadCountAtom`, `initialLoadingAtom`, `hasMoreAtom`,
  `loadingMoreAtom` are gone. Jotai now holds only genuine local-UI
  state (selection, batch mode, mobile view, filter chips, sort
  order, compose source). `store/chat.ts` renamed to
  `store/ui.ts` to reflect the new scope.
- **New primitives.**
  - `hooks/use-flat-conversations.ts::useFlatConversations(filters)`
    — identity-preserving flatten over
    `conversationKeys.infinite(filter)`.
  - `hooks/use-current-mail-filters.ts::useCurrentMailFilters`,
    `::useCurrentThreadMessages`, `::useCurrentUnreadCount` —
    the ONE way to compose atoms into a `MailListFilters`, read the
    open thread's messages, or derive an unread total. All go
    through RQ.
  - `reducers/snapshot.ts::patchAllInfiniteLists(qc, updater)` —
    walks every `conversationKeys.infinites()` cache line and
    applies a pure updater; the sole primitive for optimistic
    patches (used by the 6 keyboard shortcuts + mark-all-read + the
    reducers in `reducers/commands/conversation.ts`).
  - `wire/client.ts::wireFetch<T>(schema, req)` — the only fetch
    wrapper. Uses Zod schemas at the wire boundary for runtime
    validation.
  - `domain/ids.ts` branded ID types (`ThreadId`, `MessageId`,
    `Uid`, `AccountId`, `DraftId`, `DomainName`, `AliasAddress`)
    with narrow constructors enforced at wire parse.
- **Anti-flash discipline.** Global `queryClient` defaults include
  `placeholderData: keepPreviousData` — filter changes and page
  transitions never blank the screen.
- **Design tokens.** Three sub-`text-xs` sizes (`text-tiny` = 10 px,
  `text-mini` = 11 px, `text-mid` = 13 px) formalised in
  `index.css`; 34 inline `text-[Xpx]` literals migrated. Total
  arbitrary-value literal count: 99 → 65 (remaining are all
  documented legitimate uses — CSS-variable references, viewport
  units, animation keyframes, and `pctToWidth` bucket lookups).

### Locked RFC decisions

1. Zod runtime validation at the wire boundary.
2. `react-router` v7 data routes via `createBrowserRouter` +
   `RouterProvider` (Phase 8 — pending).
3. `openapi-typescript` output generated at build time from
   `web/public/openapi.json`.
4. `syncStoragePersister` for React Query cache persistence to
   localStorage.
5. Env-only feature flags via `__FLAGS__` vite define.

### Deployment protocol during this cycle

Per user directive 2026-07-07: every phase built locally and
direct-rsync'd to prod + staging, no CI. Only the final v2.1.0 tag
runs through `release.yml` on explicit user go-ahead.

## v2.0.0 — 2026-07-07

GA release of the Rust binary + fastcore stack. Landed at commit
`4f01bc64`. Consolidates every Stage B / C / D increment that had
been accumulating on `develop` since v1.9.4.

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
- **Stage C.1:** MCP tool surface expanded from 37 to **62** tools
  across 10 v2 batches — hits the plan target:
    - Batch 1 (admin-read): `list_groups`, `list_apps`,
      `list_email_groups`, `list_greylist_local`, `list_aliases_admin`.
    - Batch 2 (admin-misc): `reconcile_maildir`, `list_scheduled_outbound`,
      `get_email_group_members`.
    - Batch 3 (per-user outbound control): `cancel_scheduled`,
      `reschedule_scheduled`.
    - Batch 4 (self-introspection): `get_my_permissions`,
      `list_own_scheduled`.
    - Batch 5 (encryption keys): `list_own_encryption_keys`,
      `get_public_key_of`.
    - Batch 6 (admin queue): `list_admin_queue`, `list_failed_outbound`.
    - Batch 7 (server info + retry): `get_server_info`, `retry_queue_message`.
    - Batch 8 (thread summary): `get_thread_summary`.
    - Batch 9 (thread mutations): `snooze_thread`, `unsnooze_thread`,
      `pin_thread`, `unpin_thread`, `dismiss_thread_action`.
    - Batch 10 (dashboard metrics): `get_inbox_metrics`.
- **Stage C.5 (partial):** `mcp.rs` → `mcp/{mod,params,tools_v2_batchN}.rs`
  named-router split. Each new batch file <500 lines (file-size
  hard rule); the 37 legacy tools remain in mod.rs pending a
  post-v2 per-category split.
- **Upstream tracking:** kevy-client 1.13 does not wrap kevy-server's
  3.17 features (brpop / hexpire / zinterstore / idx / changes_since).
  Phase 4 (BRPOP), Phase 5 (HEXPIRE), and Phase 7/8 for network paths
  remain blocked.
- **Docs / rules:** ARCHITECTURE.md fastcore-topology refresh + crate
  count 44 → 59, README.md dropped legacy `docker compose up postgres
  kevy` (both engines in-process since v1.7.95), PERFORMANCE.md
  added a v2 kevy 3.17 refactor row with a per-site table and the
  staging soak `slow_pct` trend (0.72 % → 0.59 % → 0.67 %),
  DEPS_AUDIT.md marker for the kevy stack + kevy-client 1.13 gap
  callout, DEPLOY.md rewritten end-to-end for the release.yml + git
  flow model with a manual rollback runbook, `web/public/openapi.json`
  version 0.9.3 → 2.0.0, CHANGELOG.md (this file) established.

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
