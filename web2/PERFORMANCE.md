# web2 — performance charter

> **Performance is P0.** mailrs CRUD endpoints return in sub-millisecond
> (server-side `/api/health duration_ms=0` on v1.7.97 prod). Any
> perceptible UI slowness is engineering failure, not "expected".
>
> This document is the **frozen contract** for web2. Every PR that
> regresses any number here is rejected. Every new page must update its
> baseline section below before merging.

---

## Hard budgets — CI gates reject violators

| Metric | Budget | Measurement | CI gate |
|---|---:|---|---|
| Initial bundle (gzipped, excluding lazy chunks) | **≤ 100 KB** | `vite build` + `size-limit` | `bun run perf:bundle` |
| Per-route lazy chunk (gzipped) | **≤ 30 KB** | same | same |
| Tiptap chunk (lazy, only loaded on `/mail/compose`) | **≤ 80 KB** | same | same |
| Time to Interactive (TTI), local dev build, M-series Mac | **≤ 200 ms** | Lighthouse desktop preset | manual per page |
| Largest Contentful Paint (LCP) on `/mail` first load | **≤ 400 ms** | Lighthouse | manual per page |
| Long task duration (single task, any UI action) | **≤ 50 ms** | DevTools Performance recording | manual per page |
| React component re-render count per user action | **≤ 5** | React DevTools Profiler | manual per page |
| List virtualization threshold | **> 50 items mandatory** | ESLint custom rule + reviewer | `bun run perf:lint` |
| Bundle dependency size — any single dep | **≤ 30 KB gzipped before lazy split** | `bun run perf:bundle` weekly | weekly audit |

---

## Mandatory dependencies (project principle: dogfood GOLIA stack)

Per mailrs project principle **"dogfood 自研优先"** (memory:
`feedback-dogfood-goliapkg-priority.md`), the following GOLIA-internal
dependencies are **mandatory** in web2. Perf concerns about them are
*feedback to upstream*, never reasons to bypass:

| Dependency | Role | If perf doesn't meet budget |
|---|---|---|
| `@goliapkg/gds` | Design system, layout primitives, visual language | File perf notes to `/Users/doracawl/workspace/goliapkg/gds/.claude/notes/mailrs-*.md` with hard numbers; coordinate fix with GDS team; do **not** roll a parallel primitive set |
| (future GOLIA HTTP / state / editor libs) | Apply same principle as they ship | same |

Note: this is the **opposite** policy from "minimize bundle size at all
costs". mailrs is the production validation site for GOLIA libraries.
A bigger bundle is acceptable if it's the cost of dogfooding —
**but** the bundle baseline ledger still must track the real numbers,
and any GDS-driven exceedance triggers a feedback PR to GDS, not a
local workaround.

## Disallowed dependencies (perf debt)

| Dependency | Reason | Alternative |
|---|---|---|
| `axios` / similar HTTP libs | 13+ KB of unneeded abstraction over native fetch | native `fetch` + ≤100 LOC wrapper |
| `moment` / `dayjs` (full) | i18n locale data bloat | `Intl.DateTimeFormat` + 30-line helpers; `dayjs` minimal only if absolutely needed |
| `lodash` (full) | 20+ KB | `lodash-es` per-function imports only, or write inline |
| `react-icons` (full) | hundreds of KB | `lucide-react` already chosen, **but tree-shake per-icon imports** |
| Any styled-components / emotion variant | runtime CSS-in-JS = re-render cost | Tailwind only |
| `react-helmet` / `react-helmet-async` | runtime DOM manipulation | `<title>` via React 19 native metadata; or document.title on route change |
| `react-router-dom` v6 | older API, bundle bigger | React Router 7 (already chosen, framework mode disabled to keep simple) |
| `redux` / `redux-toolkit` | overkill for our scale | Jotai (already chosen, atoms ≤10 hard cap) |

Any PR adding a dep over 5 KB gzipped requires explicit perf-charter
addendum + reviewer approval.

---

## State source rules (hard, CI-enforced)

There are **exactly three** kinds of state in web2:

### 1. Server state → TanStack Query exclusively

* Every byte of data that came from `mailrs-server` REST or WebSocket
  goes through `@tanstack/react-query`.
* No `fetch()` call in component code. Every fetch is wrapped in a
  query hook in `src/api/`.
* Default configuration:
  ```ts
  new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: Infinity,
        gcTime: 5 * 60_000,
        refetchOnWindowFocus: false,
        refetchOnMount: false,
        refetchOnReconnect: false,
        retry: 1,
      },
    },
  })
  ```
* **No** background polling. **No** "refetch on focus". Invalidation is
  always explicit (via WebSocket event hook or mutation `onSuccess`).
* WebSocket events → one `useMailEvents` hook → calls
  `queryClient.invalidateQueries({queryKey: [...]})`. No per-component
  WS subscriptions.

### 2. Global UI state → Jotai, ≤10 atoms hard cap

The complete list of allowed atoms (extend only with PR justifying
why server state can't carry it):

```ts
// src/store/ui.ts — total ≤10 atoms
sessionTokenAtom        // bearer token; sourced from sessionStorage
currentUserAtom         // derived from sessionTokenAtom + /api/me query
themeAtom               // 'light' | 'dark' | 'system'
sidebarCollapsedAtom    // bool
listColumnWidthAtom     // user-set list column width persisted
connectionStatusAtom    // 'connected' | 'reconnecting' | 'offline'
focusedConversationAtom // current selection ID, drives thread view
composerVisibleAtom     // floating composer open/closed
// ... reserved 2 slots; need 11th → PR justification
```

CI gate: `bun run perf:atoms` parses `src/store/ui.ts`, counts
`atom<...>(...)`, fails build if >10.

### 3. Component-local state → `useState`

* Form fields before submit
* Hover / focus / open-close UI transient
* Validation errors
* That's it. Anything wider than one component → category 1 or 2.

**Cross-rule**: a piece of data **must not appear in more than one
category**. If `mailList` is in Query, it must not also be in a Jotai
atom. CI gate `perf:state-source` greps for atom names that match
query keys and fails.

---

## React rules — hard

* **No `useEffect` for data fetching**. Data fetching = TanStack
  Query. `useEffect` allowed only for: DOM measurement (e.g. resize
  observer setup) and subscription cleanup (e.g. WS connect/disconnect
  in the single events hook).
* **`useMemo` / `useCallback` default-off**. Allowed only when:
  * Prop is passed into a `React.memo`-wrapped child, AND
  * Eslint comment documents the wrapped component path.
  `bun run perf:lint` flags un-justified uses.
* **`React.memo` mandatory** for: every list-item component, every
  row component, every cell component (table cells, etc.).
* **Lists > 50 items** must use `@tanstack/react-virtual`. Custom
  ESLint rule `web2/no-non-virtual-large-list` enforces.
* **No inline arrow handlers in lists** — `onClick={() => doX(item)}`
  inside a list-item triggers re-render on every parent render.
  Pattern: use the row item's `id` as data attribute and event
  delegate on the list container.
* **No inline object/array props** to `React.memo`'d children
  (referential identity). Either lift to `useMemo` (with justification)
  or store in Jotai/Query.

---

## Render rules

* Routes are **all lazy-loaded** via `React.lazy()` + `<Suspense>`.
  Initial bundle contains only: shell, router, login, redirect logic.
* **No prerendered ghost content** on suspense fall-through. Loading
  state is a single 1-line skeleton, not a full mock layout.
* **No heavy work in render**. Sort, filter, group → memoized hooks
  *with justification*, or move to TanStack Query `select()`.
* **`<Suspense>` boundaries**: one per route, one per data-heavy
  section. Not at the leaf-component level (over-suspending).

---

## CSS / responsive rules

* Tailwind 4 utility classes only. **0 inline `style={{}}`**. ESLint
  rule `react/forbid-dom-props` configured to reject `style`.
* **Responsive design via CSS container queries (`@container`), not
  viewport media queries**. This is the direct fix for "宽度不同表现
  极差" — components respond to their *container* width, not the
  viewport. A thread panel renders the same regardless of whether
  the sidebar is open.
* **Container query breakpoints** (use these names everywhere):
  ```
  @container (min-width: 480px)  → md:
  @container (min-width: 720px)  → lg:
  @container (min-width: 1024px) → xl:
  ```
  Tailwind 4 supports container queries natively via the `@container`
  variant.
* **Spacing scale** — only Tailwind defaults: `0.5 1 2 3 4 6 8 12 16
  20 24`. No arbitrary `[7px]` values without comment justifying why.
* **Custom CSS in `src/styles/index.css` only**, expressed as
  `@layer utilities` Tailwind extensions. **No** per-component CSS
  files.

---

## P0 user-pain scenarios — derived from legacy `web/` failures

These three operations are **the highest-frequency real-user actions
in mailrs**. Every one of them is broken or slow in legacy `web/`.
web2's success is measured first by whether these three feel
instantaneous.

### Scenario A — switch between conversations in the mail list

**Legacy failure**: clicking from one conversation to another freezes
the UI for several hundred ms while the entire thread-view (1429
LOC god component) re-renders.

**web2 hard requirements**:
* URL is the source of truth: `/mail/thread/:id`. Clicking a list
  item performs `navigate(...)` only; React Router triggers the
  scoped re-render of `<ThreadPane>` only. **List pane does NOT
  re-render.**
* Thread metadata (headers, sender, subject, recipients) is fetched
  via TanStack Query keyed by `['thread', id]`. The list pane has
  no knowledge of the focused thread.
* Thread body is fetched separately via `['thread-body', id]` and
  rendered with a fixed-height skeleton while loading.
* **Interaction-to-paint budget: ≤ 50 ms** for the metadata pane,
  ≤ 200 ms for body. Profiled via React DevTools, single user click,
  cache cold.
* Switching between 2 already-cached conversations: ≤ 16 ms (single
  frame). Verified via DevTools Performance recording.

### Scenario B — search

**Legacy failure**: search is slow or "doesn't find things"
(functionality incomplete).

**web2 hard requirements**:
* Search input is debounced **150 ms** (no per-keystroke fetch).
* Server endpoint `/api/mail/search` (mapped to Meilisearch backend)
  returns in **≤ 80 ms** (server-side budget; verify
  `duration_ms` from prod logs).
* Frontend round-trip (input → results render) **≤ 200 ms p95** on
  local dev build.
* Result list **always virtualized** (assume any query may return
  thousands of hits).
* Search **never** falls back to client-side filtering of a fetched
  list. If the server can't answer, show "search unavailable"
  rather than fake it.
* Full Meilisearch feature set exposed: fuzzy match, field
  restriction (`from:`, `subject:`, `before:`, `after:`, `has:`),
  highlighting in results.
* Result item click → goto `/mail/thread/:id` → Scenario A budget
  applies for the navigation.

### Scenario C — tag (label) operations

**Legacy failure**: tag application slow, tag UI incomplete.

**web2 hard requirements**:
* Applying / removing a tag uses **optimistic update**:
  TanStack Query `mutate()` updates the cache before server confirms;
  rollback on error.
* Mutation success invalidates ONLY the affected query key (the
  one thread or message), **not** the full mail list query.
* Tag picker UI: keyboard reachable, opens with one keystroke from
  the focused conversation; closes on Esc; tag creation inline.
* Multi-select on list items + bulk tag operation in one server
  round-trip (`/api/mail/bulk-tag` endpoint — add to server if not
  present).
* Tag rename, color change, deletion: dedicated settings panel with
  cascade preview ("this tag is on N messages") before destructive
  action.
* **User-visible latency for single-tag toggle ≤ 16 ms** (optimistic
  update means it should feel like flipping a CSS class).

### These scenarios are CI/manual baselines, not aspirational

Add them to the baseline ledger immediately. Any web2 release that
regresses any number above is rejected. These three measurements
are the **first** thing measured on the first scaffolded page that
can host them (Phase 1 — mail core).

---

## Per-page performance baseline ledger

Every page added must append a row here before merging. Numbers from
Lighthouse desktop preset, local production build, M-series Mac.

| Page | Bundle (gzip) | LCP | TTI | Re-renders / action | Date |
|---|---:|---:|---:|---:|---|
| `/login` | TBD | TBD | TBD | TBD | TBD |
| `/mail` (list + thread) | TBD | TBD | TBD | TBD | TBD |
| `/mail/compose` (incl. Tiptap chunk) | TBD | TBD | TBD | TBD | TBD |
| `/settings` | TBD | TBD | TBD | TBD | TBD |
| `/admin/*` | TBD | TBD | TBD | TBD | TBD |

### P0 scenario baselines

| Scenario | Metric | Budget | Measured | Date |
|---|---|---:|---:|---|
| Switch conversation (cache cold) | Click → thread metadata paint | ≤50 ms | TBD | TBD |
| Switch conversation (cache cold) | Click → thread body paint | ≤200 ms | TBD | TBD |
| Switch conversation (cache hot) | Click → full thread paint | ≤16 ms (1 frame) | TBD | TBD |
| Switch conversation | List pane re-render count | **0** | TBD | TBD |
| Search | Input → results paint (p95) | ≤200 ms | TBD | TBD |
| Search | Result list virtualized for ≥ 50 hits | yes | TBD | TBD |
| Tag toggle (optimistic) | Click → visual change | ≤16 ms (1 frame) | TBD | TBD |
| Tag toggle | Server invalidates only affected key | yes | TBD | TBD |
| Bulk tag (10 items) | Click → all visual change | ≤16 ms (1 frame, optimistic) | TBD | TBD |

PR that pushes a number worse than the prior row for the same page is
rejected unless the PR description quantifies why and gets explicit
sign-off.

---

## Disallowed anti-patterns (PR review checklist)

* `setState` inside `useEffect` watching the prev state → infinite-render risk
* Multiple `useEffect`s for related work → consolidate into one
* `useEffect(() => fetch(...), [])` → use TanStack Query
* Component file > 300 lines → split (CI gate enforces)
* Two components reading same Jotai atom → consider Query / lift to one parent
* `key={index}` on list items → use stable id
* Wrapping a component in `React.memo` "just in case" → only when profiled bad
* `useCallback(...).filter(...).map(...)` in render → move to memoized hook with justification
* Inline `<svg>` over 50 lines → extract to component file, lazy if not on critical path
* Importing a date / utility lib for one function → write the function

---

## Measurement protocol

Before merging any UI-touching PR:

1. `cd web2 && bun run build`
2. `bun run preview`
3. Open in Chrome Incognito, DevTools → Performance → record a 5s
   capture of the touched flow
4. Take Lighthouse desktop snapshot
5. Compare to the baseline row for affected pages; if any number got
   worse, either justify in PR description or fix before merge

---

## Why these rules

Almost every rule in this doc directly counters one specific
anti-pattern observed in legacy `web/`:

| legacy `web/` smell | web2 rule |
|---|---|
| 87 `useAtom*` + 137 `useState` double-tracking server data | State source rules, 3 categories, no overlap |
| 64 `useEffect` + 18 raw `fetch()` + WebSocket hooks 3-way mix | TanStack Query exclusive; useMailEvents single WS hook |
| 81 `useCallback` mostly cargo-cult | `useCallback` default-off, justify-on |
| 26 inline `style={{}}` writing pixel widths | Tailwind-only + container queries |
| 3 god files > 1400 LOC | 300 LOC hard CI cap |
| GDS 2.2.0 imported but inline styles win | No DS; Tailwind direct |
| No bundle budget, no measurement → unknown size | Hard budgets, CI gates, baseline ledger |
| Auto-refetch on focus default → spurious work | `staleTime: Infinity`, explicit invalidation |
| Switching conversations re-renders entire god component | URL is source of truth, scoped Query keys, list pane *never* re-renders on thread change |
| Search slow or incomplete | Server-side Meilisearch only, debounced 150 ms, virtualized results, ≤200 ms p95 frontend round-trip |
| Tag operations slow / functionality gaps | Optimistic mutations, single-key invalidation, 1-frame visual update, multi-select bulk |

Every rule has a name attached to a real number from the legacy audit.
This is not premature optimization; this is documentation of *the
exact failure modes we already shipped once*.

---

## When this charter must be amended

Performance budgets get more strict over time, not looser. The only
reason to relax a number is when actual user-visible behavior is
acceptable at a higher cost AND the cost is justified by a feature
the user explicitly asked for. Generic "we needed flexibility" or
"the lib is convenient" are NOT acceptable reasons to amend.

Amendments require:
* New row in the baseline ledger showing the measured cost
* PR description explaining why
* Explicit sign-off from project owner
