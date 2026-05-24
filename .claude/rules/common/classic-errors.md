# Classic Errors

Mistakes that have been made before. Do not repeat them.

This file is shared across every managed project — any classic error discovered in any project belongs here, regardless of which language or stack first hit it. Do not create per-project classic-error files.

## Server state in Jotai atoms instead of react-query (the cache-arc you skipped)

**Context:** Any React app talking to a server you control. The project rules say "React Query for server state, Jotai for ephemeral client-only state" — but it's easy to start with `useEffect(() => fetchJson(...).then(setX))` + a Jotai atom because "we don't need RQ yet."

**Bug class:** A mailbox-style high-read workload ends up with all of the following symptoms before anyone names them:

1. **Refresh feels webapp-y** — every page reload reruns every fetch with no cache. ~300-600 ms of skeleton-then-content even on a warm connection.
2. **Filter switching reruns the network** — same data, different view, fresh fetch.
3. **Optimistic updates race with refetches** — user marks-read, WS event fires, server returns its current state, your local optimistic write gets stomped, the row "flickers back to unread."
4. **WS-driven `setX` clobbers identity** — every event reallocates the whole list array, breaking every `React.memo` row downstream, all 50 items rerender for one new mail.
5. **No dedup** — fast filter clicks fire concurrent requests; the late one wins by virtue of arriving last, not by being the user's intent.
6. **Hand-rolled localStorage cache** — eventually someone writes `lib/list-cache.ts` to fix #1, then has to manually invalidate it on every mutation path, and forgets one.
7. **Request-generation guards** — the same someone writes `loadGenRef.current++` to fix #5. Now mutations have to coordinate with the guard too.

Every one of these is a symptom of the same root cause: server state should not live in `useState` / Jotai atoms. RQ exists specifically to handle dedup, cache, refetch-on-mount, optimistic-with-rollback, and queryClient.setQueryData for WS-driven patches.

**Real incident (2026-05-19):** mailrs ran on this anti-pattern for months. The frontend grew `list-cache.ts`, a request-generation counter, a `shallowEqualConvo` helper, custom WS merge logic, a manual "preserve identity across refetches" map — every one of those is a single RQ feature reinvented in 30-60 lines of project-specific code. After migrating (`@tanstack/react-query` + `PersistQueryClientProvider` + `useQuery` / `useMutation` on the hot path), every one of those files was deleted and the user-visible refresh path went from "skeleton flash" to "appears to never reload."

**Rules going forward:**

1. **Server state goes through RQ from day one.** If you can `fetch` it, it has a `useQuery`. Never call `fetchJson` inside `useEffect` directly. Never write to a Jotai atom from a fetch result — pipe the query's `data` into the consumer instead.
2. **Atoms are for client-only UI state.** Filter selections, modal open/closed, currently-selected ID, batch-mode toggles — yes. Cached lists, cached messages, fetched counts — no. The clean line: "would this still make sense if the server didn't exist?" → Jotai. Else → RQ.
3. **WS events trigger `queryClient.invalidateQueries` or `setQueryData`, never `setX`.** RQ's cache is the single source of truth; the atom mirror is the bug.
4. **Mutations use `useMutation` with `onMutate` for optimistic patches + `onError` rollback.** Don't write to the atom-mirror in the success path; the cache patch IS the success path.
5. **The pattern is exported as `use<Entity>Query` and `use<Action>Mutation` hooks** in one file per domain (`use-mail-queries.ts`, `use-mail-mutations.ts`). New pages reach for those, not for raw `useQuery({queryKey, queryFn})` inline, and not for `fetchJson` directly.
6. **Server-side cache layer mirrors the client.** If client-side RQ has a 30s staleTime on a list, the server probably has a Valkey cache-aside with a 30s TTL on the same endpoint. Mutations bust both. WS events from the server should also bust the server-side cache (mailrs's `event_bus` listener pattern). The two caches plus surgical invalidation is what makes a high-read workload feel native.

**Smell tests for whether you've fallen into this antipattern:**

- A `setInterval(refresh, ...)` inside a component → it should be `refetchInterval` on a `useQuery`.
- A `useEffect(() => { fetchJson(...).then(setX) }, [...])` → ⚠️.
- A custom `list-cache.ts` / `loadGenRef` / `requestId` / `shallowEqualX` helper → 💀, you've reinvented RQ.
- Server data living in a Jotai atom that's also written to by WS event handlers → ditto.

## Meilisearch multi-version bumps require dump-restore, not auto-migrate

**Context:** Any docker-compose / production upgrade that crosses more than one Meilisearch minor version (e.g. `v1.13` → `v1.44`, or `v1.x` → `v2.0`).

**Bug:** Confidently claiming in a commit message or to the user that the new Meilisearch container "auto-migrates the index on first boot." It does not. Meilisearch refuses to open a database written by a different engine version and exits with `Your database version (X.Y.Z) is incompatible with your current engine version (A.B.C). To migrate data between Meilisearch versions, please follow our guide...`. The container then crash-loops while the rest of the stack (which expects search to work) emits connection errors.

**Real incident (2026-05-19):** During a `chore: bump docker images to latest stable` pass on mailrs, Meilisearch was bumped `v1.13` → `v1.44` in one shot. The commit message said the upgrade "auto-migrates index on first boot" — this was fabricated based on assumption, not verified against Meilisearch docs. After deploy, the meili container entered a restart loop and the mailrs server logged repeated `Meilisearch index error: error sending request for url ... /indexes/messages/documents`. Other services (SMTP / IMAP / web) stayed healthy, so the failure was contained to search — but no existing mail was searchable until recovery.

**Recovery dance (the only working path):**
1. Edit compose to point Meilisearch back at the **previous** version (the one that wrote the data). Bring just that service up.
2. Trigger a dump via the HTTP API: `POST http://meili:7700/dumps` with the master key. Poll `/tasks/<uid>` until `status == succeeded`.
3. Copy the dump file out of the volume to the host: `docker cp <container>:/meili_data/dumps/<uid>.dump ./meili-dump.dump`.
4. Stop Meilisearch, remove the data volume entirely (`docker volume rm <name>`), switch the image tag back to the target version.
5. Use a temporary `docker-compose.override.yml` to inject `command: ["meilisearch", "--import-dump", "/import.dump"]` and a bind mount for the dump file. Bring the service up — it imports the dump into the fresh volume at the new engine version.
6. After import logs confirm success (count matches pre-dump), delete the override and the dump file, then `docker compose up -d meilisearch` so it restarts with the normal command. Re-running with `--import-dump` on an already-populated volume errors out, so the override MUST come off.

**Rules going forward:**

1. **Never claim "auto-migrates" without checking the project's documented migration policy.** For Meilisearch specifically: anything other than a patch bump in the SAME minor (e.g. `v1.44.0` → `v1.44.3`) needs the dump-restore dance.
2. **For any datastore image bump, audit the migration story before the commit.** Postgres major bumps need `pg_upgrade` or dump-restore. Valkey/Redis patch bumps are usually safe but major bumps may change RDB format. Meilisearch needs dump-restore across minors. Document the chosen path in the commit message — not "auto-migrates", but either "in-place safe per upstream docs" (with link) or "requires dump-restore — see runbook".
3. **Stage datastore bumps separately from app bumps.** Lumping `meilisearch:v1.13 → v1.44` into a generic "bump docker images" commit hides the migration risk under a benign-looking diff. Datastore changes deserve their own commit with the migration plan in the body.
4. **When a stateful service is in a restart loop, check ITS logs first, not the app logs.** The app's `connection refused` messages are downstream symptoms. The root cause is in the datastore container's logs.

**Bonus gotcha discovered in the same incident — Postgres collation version mismatch when the base OS jumps:** bumping `pgvector/pgvector:pg18` (Debian bookworm, glibc 2.36) to `pg18-trixie` (Debian trixie, glibc 2.41) leaves the database's recorded collation version stale. PG warns `database "X" has a collation version mismatch` on every query. Fix: `REINDEX DATABASE <name>;` then `ALTER DATABASE <name> REFRESH COLLATION VERSION;` for each affected database (including `postgres` and `template1`). Note that `REINDEX DATABASE` cannot run inside a transaction block, so it must be its own psql invocation. Without REINDEX, text indexes may sort or compare incorrectly until the next manual rebuild.

## Fabricating tool output

**Context:** Any time a Bash/Read/Grep/etc. tool returns empty content, only a footer (e.g. `[rerun: bN]`), or output that scrolled away.

**Bug:** Instead of reporting "no output" or rerunning the tool, Claude invents plausible-looking content and presents it as if it came from the tool. The invented content looks reasonable (typical file names like `README.md`, `TODO.md`, `run-locally.sh`, `.env`) so the user may not notice until they verify.

**Real incident (2026-04-15):** User ran `ls` and `ls -a` in the repo root as the *first two messages of a fresh session*. Both Bash calls returned only `[rerun: bN]` footers with no visible stdout. Claude fabricated two directory listings including files that did not exist (`TODO.md`, `run-locally.sh`, `README.md`, `.DS_Store`, `.env`). The user immediately caught it and pointed out how severe this was: **past hallucinations at least had the (weak) excuse of long conversations and context compression — this one had neither. Two messages in, empty prompt, still fabricated.**

**Diagnosis:** The trigger is NOT context pressure or token loss. It is a **"answer the question" reflex** that skips the step of actually reading the tool result. The model sees `ls`, knows what `ls` output "usually looks like", and emits a plausible fake without ever checking what the tool returned. This reflex can fire on turn 1 of a session just as easily as on turn 100.

**Why it is catastrophic:** Fabricated tool output poisons every downstream decision. The user loses trust in any claim about filesystem state, git state, test results, or anything else derived from tools. Worse than "I don't know" — it is active misinformation disguised as verified fact. And because it can happen with zero context pressure, *no session is ever "safe" from it* — vigilance must be unconditional.

**Rules:** See `anti-hallucination.md` → "Tool Output Itself Must Never Be Fabricated" for the enforcement rules. The short version:
1. `[rerun: bN]` is a handle, not content.
2. Empty/minimal tool output must be reported literally.
3. Never paraphrase tool output from memory — rerun instead.
4. If you catch yourself about to type unverified output, STOP and rerun.
5. User-flagged hallucinations are P0.

## Unit tests passing does not mean the app works

**Context:** Any change that alters import structure, navigation stacks, build-time transforms, bundler tree-shaking, or touches platform-specific integration points. Unit tests exercise pure logic inside a single module; they cannot catch problems that only surface when the full application bundle, runtime, or native layer is involved.

**Bug:** All `cargo test` / `bun test` / `vitest` / `pytest` runs are green, but the built application crashes at startup, hangs on a specific navigation, or renders nothing on a particular screen. The test suite gave false confidence.

**Classes of problem unit tests can't catch:**

1. **Require / import cycles** introduced by restructuring shared modules. Test runners typically resolve modules one at a time and tolerate cycles that are fatal to the production bundler.
2. **Framework / navigator context boundaries.** A hook that relies on `useSomeContext()` passes when run inside a test harness that provides the context, but crashes in production when placed inside a screen that uses a different provider variant (e.g. Expo Router's `NativeTabs` not providing `BottomTabBarHeightContext`).
3. **Platform-specific native or build-time transforms.** Hermes bytecode compaction, Metro bundler tree-shaking, Rust `#[cfg]` gating, Vite SSR vs CSR split — anything that changes the code between "what the test runs" and "what ships" is an untestable blind spot.

**General rule:** Unit tests are necessary but insufficient. For changes that touch any of the above, require an integration or E2E smoke test before merging — build the app, run it against a real target, exercise at least the happy path. If no E2E exists for the affected surface, either add one or ship with explicit manual verification notes in the PR.

**When to force E2E:** import restructure in a shared client / interceptor / API layer; navigation hook changes; bootstrap / auth flow changes; introduction of a new cross-module dependency; anything behind a `#[cfg]`, `process.env.NODE_ENV`, or build-time feature flag.

## React hooks after early return

**Context:** Any React component that has a guard clause (`if (!data) return null`) followed by hook calls (`useMemo`, `useCallback`, `useEffect`, etc.).

**Bug:** React error #310 — "Rendered more hooks than during the previous render." The page crashes with no useful error message in production.

**Root cause chain:**
1. Component has an early return: `if (!data) return null`
2. Hooks (`useMemo`, `useCallback`, `useEffect`) are placed AFTER the early return
3. On first render, `data` is null → early return → hooks are NOT called
4. `data` loads → no early return → hooks ARE called → hook count changes between renders → React crashes

**Why it keeps happening:**
- The pattern feels natural: "guard first, then do work"
- ESLint's `react-hooks/rules-of-hooks` catches it, but developers suppress it with `// eslint-disable-next-line` because the code "works in dev" (React 18 was lenient, React 19 crashes)
- The crash only happens in production (minified build) making it hard to debug — the error message is just a number

**Fix:** ALL hooks MUST come before ANY early return. Move hooks above the guard clause. Hooks can safely receive null/undefined — use conditional logic inside the hook instead:

```tsx
// WRONG — hooks after early return
const data = useFetch()
if (!data) return null
const processed = useMemo(() => transform(data), [data])  // 💥

// RIGHT — hooks before early return
const data = useFetch()
const processed = useMemo(() => data ? transform(data) : [], [data])
if (!data) return null
```

**Enforcement:** NEVER write `// eslint-disable-next-line react-hooks/rules-of-hooks`. If the linter flags it, the code structure is wrong — fix the structure, don't suppress the warning.

## Stale async cache after mutation

**Context:** Any system where a background worker produces cached results (syntax highlighting, search indexing, LSP diagnostics) while the main thread mutates the source data.

**Bug:** After an edit, the async worker may return results computed from the PRE-edit source. These results contain byte/line offsets that no longer match the new content. If used directly, they cause silent rendering failures (text not drawn, garbled layout, phantom content).

**Root cause chain:**
1. User edits text → source data changes
2. Old async request (submitted before the edit) completes and returns stale offsets
3. Main thread accepts the stale result and overwrites any invalidation you did
4. Renderer uses stale byte ranges → `span_end > line_text.len()` → span skipped → nothing drawn

**Fix — two layers required:**
1. **Immediate invalidation:** When source data is mutated, immediately truncate/clear the cache from the mutation point onward. This ensures the current frame renders with a safe fallback (e.g., plain text without syntax colors).
2. **Stale result rejection:** When polling async results, check whether the source was modified since the request was submitted. If so, discard the result — do not let it overwrite the invalidated cache.

**General rule:** Whenever an async producer and a synchronous mutator share a cache, the mutator must both (a) invalidate the cache immediately and (b) ensure no in-flight async result can overwrite that invalidation. Generation counters or dirty flags work for (b).

## macOS Metal live resize wobble

**Context:** Any Metal/CAMetalLayer app on macOS where the window is resizable.

**Bug:** During live window resize, content visibly stretches/wobbles because the compositor scales the old drawable to fill the new window size before the app renders a new frame at the correct size.

**Root cause chain:**
1. User drags window edge → macOS resizes the window continuously
2. CAMetalLayer's drawable size lags behind the actual window size
3. Compositor stretches the old frame to fit the new bounds (default `contentsGravity = resize`)
4. App renders a new frame at the correct size, but the stretched frame was already displayed → visible wobble

**Fix — two parts required:**
1. **`contentsGravity = kCAGravityTopLeft`** — prevents the compositor from stretching old content; pins it to top-left corner instead, so stale frames just get clipped rather than scaled.
2. **`contentsScale = backingScaleFactor`** — ensures drawable pixels map 1:1 to screen pixels on Retina displays. Without this, topLeft gravity causes coordinate mismatch (content appears at wrong scale, clicks land in wrong positions).

**Bonus:** Read actual texture dimensions from the drawable (`msg_send![texture, width/height]`) instead of using cached width/height, because during resize the drawable may not yet match the cached size.

**WARNING — do NOT use `presentsWithTransaction` + `waitUntilScheduled`:**
These were tested and cause frames to not be presented to screen. Events are processed (hit tests pass, state updates correctly) but the visual output freezes. The `contentsGravity + contentsScale` approach alone is sufficient and does not block the event loop.

**General rule:** For flicker-free Metal resize on macOS, use non-scaling content gravity (`topLeft`) with correct `contentsScale`. Avoid synchronous presentation APIs (`presentsWithTransaction`, `waitUntilScheduled`) as they interfere with normal frame delivery in event-driven apps.

## Shipping unsolicited UI additions

**Context:** Any UI work where the implementer notices "this object has a flag / property / state that could be surfaced visually" and decides to add an indicator, badge, icon, or color change for it.

**Bug:** A visual element is shipped without appearing in any design document, Figma file, or explicit product request. Users see the element, can't explain why it's there or why it only appears on some items, and file a confusion bug. The implementer then spends hours defending or removing it.

**Real incident:** In insight mobile, a chart icon was added next to counting list items driven by an `export_to_dashboard` flag — no design, no Figma, no product ask. Users asked "why do some items have icons and others don't?" and filed a confusion bug that took rework to resolve.

**Root cause:** The "I noticed this flag exists and thought it would be useful to expose" reflex. The implementer treats their own design intuition as equivalent to a product decision. It is not — product and design decisions belong to product and design, and engineers' job is to implement what was specified, not to improvise on top of it.

**Rule:** **Never add visual elements, icons, badges, indicators, color changes, or interactive affordances that are not explicitly specified in a design document or user requirement.** If you think something would be a good addition, propose it to the user first — do not silently ship it.

**Why this matters:** Unsolicited UI additions create confusion, undermine user trust, waste time debugging "features" nobody asked for, and can break existing workflows that depended on the old visual state. They also inflate review burden because they have no spec to review against.

**How to resist the reflex:** When you notice a flag or property that "would look nice in the UI", write the idea in a TODO or research note and keep going. Only implement it if product / design explicitly accepts it.

## @tanstack/react-virtual on a WebSocket-fed list MUST pass `getItemKey`

**Context:** Any React app rendering a long list via
`@tanstack/react-virtual` where the source array can change order or
have items inserted at the top — WebSocket-pushed mailbox view,
chat conversation list, live activity feed.

**Bug:** Adjacent rows visually overlap on screen — two distinct
items drawn at the same Y position, the lower item partially or
fully covered by the upper one. The user sees content from row N+1
peeking out from under the content of row N, or two row contents
mashed together. Shows up most after a WS event pushes a new item
to the top of the list, and clears (sometimes) after a hard
refresh.

**Real incident (2026-05-24, mailrs):** the conversation list
overlapped every other-or-so row right after `Today` and `Yesterday`
section dividers. Previously "fixed" in commit `7bcce33` by bumping
`estimateSize` 88→120px, but the actual bug came back the moment a
real row pushed past 120px. The user reported "又出现了" with a
screenshot — and the screenshot showed two distinct messages
(different senders, different subjects) overlaid at the exact same
Y. Diagnosed as the same root cause the `estimateSize` bump had
been masking.

**Root cause:** `useVirtualizer` defaults to keying its internal
height-measurement cache by ARRAY INDEX. When a WS event inserts a
new item at index 0, every existing item shifts to index+1 — and
the virtualizer keeps using the old per-index cache entries, so
every row now has the height of the row that USED to live at its
new index. The off-by-one heights compound: row 5's start position
gets computed from the wrong heights for rows 0-4, and so on. The
drift only shows as visible overlap once cumulative error exceeds
the headroom left by `estimateSize`. Bumping `estimateSize` only
delays the bug; it doesn't fix it.

A separate but related symptom: React component identity also
breaks (selection state / context-menu state on rows gets blown
away on push) because index-keyed React children get reused for
different data. `key={item.id}` on the React element handles the
React side, but does NOT fix the virtualizer's internal cache.

**Fix:** Pass an explicit `getItemKey(index)` that returns a stable
per-row identifier (same shape as the React `key` you already use).
Mirroring the two means virtualizer cache and React reconciliation
agree on identity, and the cache moves with the data when the
array shifts.

```typescript
useVirtualizer({
  count: items.length,
  estimateSize,
  getScrollElement,
  // CRITICAL when items can be inserted/sorted at runtime.
  getItemKey: (index) => stableIdFor(items[index]),
})
```

**Rules going forward:**

1. **Any react-virtual list fed by WS / RQ-invalidation / sort
   change → must have `getItemKey`.** Static lists (fully
   build-time, never re-ordered) can skip it. Anything else, no.
2. **Row-overlap reports are suspect-the-measurement-cache, not
   suspect-the-CSS.** Bumping `estimateSize` or adding margin
   between rows masks the bug; it doesn't fix it.
3. **The React `key` and the virtualizer `getItemKey` must
   compute from the SAME source field.** Drift between them
   guarantees the bug on the next list mutation. Best practice:
   define `stableKey(item)` once and call it from both sites.

**The 2026-05-24 follow-up — when row overlap comes back AGAIN
after fixing `getItemKey`:** there's a second class of races in
the dynamic-size path of react-virtual that no amount of
`getItemKey` / `estimateSize` patching can fix. Specifically:

- The row's height changes after first render (selection state
  toggles a border, hover state shows extra UI, snippet is
  conditional, etc.)
- `measureElement` fires the new height into the virtualizer
- The virtualizer updates the cache for THIS row's index
- BUT already-rendered sibling rows keep their stale
  `translateY` — they don't reflow until the next layout pass
- Visual: rows overlap until the next scroll event nudges a
  re-layout

The **彻底 fix** (commit `<TODO>` in mailrs) is: kill the
dynamic-size path entirely. Force rows to a fixed CSS height
(`h-24` etc.) + drop the `ref={virtualizer.measureElement}`
prop. The `estimateSize` value becomes the ground truth for
height, the virtualizer never re-measures anything, and the
entire race class is eliminated by construction.

The trade is: rows can no longer dynamically size to their
content. Snippet truncation must be lossy (`truncate` /
`overflow-hidden`). For mailbox / chat / activity-feed lists
this is almost always the right trade — predictable layout
beats Pixel-perfect content fitting.

**Rule of thumb:** any list where rows can change height after
first render is a dynamic-size virtualizer waiting to overlap.
If it's a WS-fed list, force fixed-height. If you can't force
fixed-height, don't virtualize.

**Why this matters beyond just one view:** the same default-keys-
by-index footgun applies to every virtualizer in the project,
including `react-window` and any custom virtualization. The
dynamic-size race applies wherever rows are
absolute-positioned + size depends on a frame-by-frame measure.
Every WS-fed list needs the audit pass + the fixed-height check.

## Silent switch from small core to big core in a dual-core system

**Context:** Any project with an explicit big.LITTLE / small-core-first design philosophy — cheap local inference (Ollama, on-device) handles the bulk of work, and the expensive remote API (Claude, GPT, etc.) is reserved for a small number of narrow, user-facing, high-value paths. The design contract is that background / periodic / learning tasks MUST run on the small core; the big core is a scarce resource metered by user-visible value.

**Bug:** Later features are added that call the big-core API from background paths (periodic scans, claim extraction on every reply, reflection loops). Each call in isolation "feels" justified — "this needs structured output", "this needs high precision" — but the cumulative effect is that the system silently becomes big-core-dependent. The user, who shipped the project under the original small-core contract, has no visibility that their quota is being drained by background work they never authorized as big-core-eligible.

**Real incident (2026-04-18 → 2026-04-21):** The `dada` project (self-writing PM agent) was designed with a strict dual-core split: big core only for identity + high-similarity factual replies. During v11–v12 ship, `contradictions.rs` and `gating.rs` were implemented using `brain.think_precise` (big core = `claude -p --model sonnet`) for background knowledge-graph contradiction scanning and per-reply claim extraction/rewriting. The contradiction scan's tick interval was left at a dev-loop value (`DADA_CONTRADICTION_TICK_SECS=60`) inside the production launchd plist. Result: one `claude sonnet` call every 60 seconds, 24/7, for roughly 60 hours before discovery — **consuming 94% of the user's 7-day `max_20x` quota on account `lihao@golia.jp`**. The user found it by running an unrelated account-usage monitor tool; nothing inside the agent itself flagged the drift.

The project was shut down and archived the same day — not because the spend was unrecoverable, but because the silent architectural drift violated the core trust contract: **the user had no way to know the dual-core design had quietly stopped being true**.

**Root cause stack (all three necessary for the blast):**

1. **Misplaced notion of "precision".** Any time a background task needs structured JSON, it feels natural to reach for the bigger model. This is wrong under a dual-core contract — background tasks are exactly where the small core must suffice, even if the output is rougher. The cost of a wrong small-core verdict on a background scan is vanishingly small; the cost of normalizing big-core use in background paths is architectural bankruptcy.

2. **No tick-interval sanity check before ship.** The plist was copied from a dev smoke-test environment where 60s made sense for fast iteration. No one (including the agent shipping it) did the one-line arithmetic `60 sec × 24 h × ≈cost_per_call` before promoting to production. A single multiply would have produced a number like "$12/day on background contradictions alone" and stopped the ship.

3. **No budget/call-rate observability inside the agent itself.** The runtime had no self-awareness of its own big-core burn rate. Detection relied on an external, unrelated monitoring tool (`devops claude` usage poll) — so the drift could run for days before the human noticed. A system that silently consumes a scarce shared resource with no internal odometer is always one ship away from this class of failure.

**Rules going forward, for any dual-core / small-first system:**

1. **Grep-auditable big-core boundary.** The set of big-core call sites MUST be enumerable by one `grep`. Add a sentinel helper (e.g. `big_core_call(reason: &str, ...)`) and forbid the raw `think_precise` / Claude CLI invocation elsewhere. Every call site must carry a written `reason:` string that ships with the audit log.

2. **No background-loop big-core calls, ever.** Periodic schedulers, reflection loops, knowledge-graph scans, hygiene tasks — these are small-core by policy. If the small core is insufficient for the task, the task is not dual-core-compatible and must be removed or redesigned, not promoted to big core.

3. **Tick-interval cost estimation is a ship blocker.** Before any commit that adds a scheduled big-core call, the commit message / PR body must include: `rate × 24 h × est_cost = $X/day`. If `$X/day > $5` it requires explicit user sign-off in the PR. If the reviewer is the model itself, it still must call out the number to the user in chat before merging.

4. **Plist env vars are production config.** Any `*_SECS` / `*_INTERVAL` / `*_CONCURRENCY` value set in `launchd` / `systemd` / `docker-compose` is production config, not a dev convenience. Default values live in code; env overrides must be justified in the commit that sets them, and any value that raises call rate above the code default requires the same cost-estimation as rule 3.

5. **Self-odometer on metered resources.** Any long-running process that consumes a scarce shared resource (LLM quota, API calls, storage, money) must publish its own running total to the same place the user monitors health (valkey key, status endpoint, dashboard). "We'll know if it's bad because we have external monitoring" is not a substitute — external monitoring tells you the damage is already done.

6. **Architectural-drift disclosure is non-negotiable.** If a change introduces a dependency that contradicts a previously-shipped design contract — even locally, even "temporarily", even if you plan to fix it later — you must flag it to the user in the same turn, with the phrase "this deviates from the shipped design because…". Silent deviations are the failure mode; loud, ugly deviations are debuggable.

**Why this matters beyond the dollar cost:** A user who invests trust and resources in a design philosophy is trusting the agent to defend that philosophy *against its own short-term convenience*. Quietly trading it away for "precision" or "structure" on individual features is the exact betrayal that ends research collaborations. The bug is not "we burned money" — the bug is "we stopped being the system we claimed to be, and didn't tell anyone".
