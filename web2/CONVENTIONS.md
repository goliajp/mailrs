# web2 — conventions

> **`web2/` has no code.** This directory contains these two documents and
> nothing else — the rewrite they describe was never started, so every
> `src/…` and `scripts/…` path below refers to files that do not exist.
> The shipping frontend is `web/`, and its conventions are in
> `.claude/rules/frontend-css.md` and `.claude/rules/frontend/*.md`.
>
> Kept as a design record. Delete it if the rewrite is off the table.

> Sibling to `PERFORMANCE.md`. Where PERFORMANCE.md says **why** and
> **how much**, this doc says **how we write code**.

---

## Project principle: dogfood GOLIA stack

mailrs is the production validation site for GOLIA-internal Rust /
TypeScript libraries (kevy, SPG, GDS, future libs). When a GOLIA stack
exists, **using it is mandatory**, not a preference:

- `@goliapkg/gds` for design system / layout primitives / visual language
- Any future GOLIA web library that overlaps with our needs

If a GOLIA library underperforms web2's `PERFORMANCE.md` budgets:
1. **Do not** roll a parallel implementation
2. **Do** write a numbers-attached feedback note to the GOLIA team
3. **Do** track the gap in our baseline ledger as a known divergence
4. **Do** coordinate fix timeline

The legacy `web/` had `@goliapkg/gds: 2.2.0` installed but bypassed it
with 26 inline styles. web2 does not repeat that mistake.

See memory: `feedback-dogfood-goliapkg-priority.md`.

---

## Project layout

```
web2/
├── PERFORMANCE.md           ← perf charter, hard budgets, baselines
├── CONVENTIONS.md           ← this file
├── package.json             ← deps, scripts
├── tsconfig.json            ← strict, no any, noUncheckedIndexedAccess
├── vite.config.ts           ← Vite + plugins
├── tailwind.config.ts       ← Tailwind 4 config (minimal; container queries)
├── eslint.config.js         ← flat config; perf rules; no inline style
├── prettier.config.js       ← shared with web/
├── vitest.config.ts         ← unit + component tests
├── public/
│   └── openapi.json         ← symlink → ../web/public/openapi.json
├── scripts/
│   ├── check-file-size.ts   ← reject > 300 LOC
│   ├── check-atoms.ts       ← reject > 10 atoms
│   └── check-bundle.ts      ← reject bundle over budget
├── src/
│   ├── main.tsx             ← entry, no logic
│   ├── app.tsx              ← <QueryProvider> + <Router> + <AuthGuard>
│   ├── routes.tsx           ← lazy route table
│   ├── api/                 ← TanStack Query hooks, one file per resource
│   │   ├── auth.ts          ← useLogin, useLogout, useSession
│   │   ├── mail.ts          ← useMailList, useThread, useSendMail, ...
│   │   ├── admin.ts         ← admin hooks
│   │   └── types.ts         ← codegen'd from openapi.json
│   ├── pages/               ← one folder per route; index.tsx ≤ 300 LOC
│   │   ├── login/
│   │   ├── mail/
│   │   ├── settings/
│   │   └── admin/
│   ├── components/
│   │   ├── layout/          ← AppShell, Sidebar, ListPane, ThreadPane
│   │   ├── primitives/      ← Button, Input, Modal, Avatar, Badge
│   │   └── mail/            ← MailListItem, ThreadMessage, Composer (lazy)
│   ├── hooks/
│   │   ├── use-mail-events.ts  ← single WS hook
│   │   ├── use-container-query.ts ← @container helper
│   │   └── use-virtualizer.ts  ← @tanstack/react-virtual wrap
│   ├── store/
│   │   └── ui.ts            ← ALL jotai atoms; ≤10
│   ├── lib/
│   │   ├── fetch.ts         ← ≤100 LOC native fetch wrapper
│   │   └── format.ts        ← Intl-based date / number helpers
│   └── styles/
│       └── index.css        ← Tailwind + @layer utilities; no other CSS file
└── tests/
    └── e2e/                 ← Playwright (Phase 2+)
```

**Hard rules**:
- One folder per page under `src/pages/`
- One file per resource under `src/api/`
- `src/components/` only contains reusable; one-off goes in the page folder
- No file outside `src/styles/index.css` may contain CSS; no `*.module.css`

---

## File size

* **Hard cap 300 LOC** per `.tsx` / `.ts` file, including imports and
  blank lines. CI gate (`scripts/check-file-size.ts`) rejects PRs that
  introduce or grow files past this.
* Justified carve-outs: codegen output (`src/api/types.ts`), test data
  fixtures (rare). Each carve-out marked with `// CODEGEN:` or
  `// CARVE-OUT: <reason>` at file top.
* If a component is approaching 300 LOC, split *before* hitting the
  cap. Extract: sub-components, custom hooks, helper functions, type
  definitions to sibling files.

---

## Naming

* **Files**: `kebab-case.tsx` (`mail-list-item.tsx`). One default
  export per file matching the file name.
* **Components**: `PascalCase` (`MailListItem`).
* **Hooks**: `use-thing.ts` files exporting `useThing()`.
* **Atoms**: `<name>Atom` suffix, all in `src/store/ui.ts`.
* **TanStack Query hooks**: `use<Resource><Action>()`, e.g.
  `useMailList`, `useSendMail`. Query keys mirror hook names:
  `['mail-list', filters]`, `['thread', threadId]`.
* **No prefixed `T` on types** (`MailListItem`, not `TMailListItem`).
* **No `I` prefix on interfaces** — prefer `type` aliases over
  interfaces; use `interface` only for external API ABI extension.

---

## TypeScript

```json
// tsconfig.json — non-negotiable
{
  "compilerOptions": {
    "strict": true,
    "noImplicitAny": true,
    "noUncheckedIndexedAccess": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "exactOptionalPropertyTypes": true,
    "noFallthroughCasesInSwitch": true,
    "forceConsistentCasingInFileNames": true,
    "jsx": "react-jsx",
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler"
  }
}
```

* `any` is banned. `unknown` instead.
* `as` casts allowed only after a `typeof` / `in` narrowing or with a
  `// SAFETY: <reason>` comment.
* Discriminated unions over enum-with-string for state machines.
* `// @ts-expect-error` allowed (forces removal once the error is
  gone). `// @ts-ignore` banned.

---

## Components

* **One responsibility per component**. If you name a component
  "MailListPanel" and inside it does selection, filtering, and
  composer-opening, split.
* **Props**: ≤6 props per component. More → split or use a single
  config object prop.
* **No prop drilling beyond 2 levels**. If a leaf needs data from
  3+ levels up: either lift the leaf out (it doesn't belong at that
  depth), or use a Jotai atom (only if the data is truly UI-global
  and approved for the ≤10 atom budget).
* **No `children: ReactNode` "slot" components** unless they're
  Layout components (`AppShell`, `Modal`, `Sidebar`). Business
  components have typed props.
* **Memo wrapping** for list items / cells: mandatory. For others:
  only after profiling shows it matters.

```tsx
// canonical list item shape
import { memo } from 'react'

export type MailListItemProps = {
  id: string
  subject: string
  sender: string
  unread: boolean
}

function MailListItemImpl({ id, subject, sender, unread }: MailListItemProps) {
  return (
    <article
      data-mail-id={id}
      className={[
        'flex gap-3 px-4 py-2',
        'border-b border-neutral-200',
        unread ? 'font-semibold' : 'text-neutral-600',
      ].join(' ')}
    >
      <span className="truncate">{sender}</span>
      <span className="truncate flex-1">{subject}</span>
    </article>
  )
}

export const MailListItem = memo(MailListItemImpl)
```

Note the **data-attribute pattern** — selection handled by parent's
event delegation, not per-item closure handler. This is the
canonical fix for "inline closure in list" perf trap.

---

## Hooks

* Custom hooks live in `src/hooks/`. One concept per file.
* Hooks compose; no hooks calling each other across module boundaries
  in subtle ways.
* No hook returns more than 3 values. Use named object return for
  more.

---

## Tests

* Every component (not page) ships with a `.test.tsx` covering:
  1. Happy render with required props
  2. Empty / loading / error states (if relevant)
  3. User interaction → expected callback / Query invalidation
* Pages get one smoke test that mounts them with a mocked QueryClient
  and checks they render without crash on first paint.
* Vitest + Testing Library. **No `act()` wrappers** in tests — if you
  need `act()`, you're testing state badly.
* **No snapshot tests**. Snapshot tests fossilize markup and rot.
  Instead: query by role / text / data-testid.
* Coverage threshold (CI gate): `≥80% lines` on `src/components/` and
  `src/hooks/`; `≥60%` on `src/pages/`.

---

## API integration

* Server REST surface lives at `mail.golia.ai/api/*`. Vite dev proxy
  rewrites `/api/*` → `http://localhost:3200`.
* Types come from `public/openapi.json` (symlinked from sibling
  `web/public/openapi.json`) via `openapi-typescript` codegen at
  `bun run codegen`. Output committed to `src/api/types.ts`.
* Every endpoint gets:
  ```ts
  // src/api/mail.ts
  export function useMailList(filters: MailFilters) {
    return useQuery({
      queryKey: ['mail-list', filters],
      queryFn: () => fetchJson<MailListResponse>(
        '/api/mail/list',
        { method: 'POST', body: JSON.stringify(filters) },
      ),
      // staleTime: Infinity inherited from QueryClient default
    })
  }
  ```
* **No** raw `fetch` calls in pages/components.
* WebSocket events:
  ```ts
  // src/hooks/use-mail-events.ts
  // single WS subscription; on event, invalidate matching query keys
  useEffect(() => {
    const ws = new WebSocket(...)
    ws.onmessage = (e) => {
      const evt = JSON.parse(e.data)
      switch (evt.kind) {
        case 'mail.new':
          queryClient.invalidateQueries({ queryKey: ['mail-list'] })
          break
        // ...
      }
    }
    return () => ws.close()
  }, [queryClient])
  ```

---

## Routing

* React Router 7, **declarative mode** (not framework mode — too
  heavy for our scope).
* All routes lazy-loaded via `React.lazy()`. Initial bundle contains
  only login + redirect glue.
* `AuthGuard` wraps non-public routes. Reads `sessionTokenAtom`; if
  empty, redirect to `/login?next=<encoded>`.
* No nested route component wrappers more than 2 deep.

---

## State source matrix (canonical)

When deciding where a piece of data lives, apply this matrix:

| Piece of data | Lives in |
|---|---|
| List of mails, threads, messages, accounts, anything from server | **TanStack Query** |
| Mutation status (sending, error) | TanStack Query mutation state |
| Current bearer token | `sessionTokenAtom` (Jotai) |
| Current user info | derived atom from token + `/api/me` Query |
| Theme preference | `themeAtom` (Jotai, persisted to localStorage) |
| Sidebar collapsed state | `sidebarCollapsedAtom` (Jotai) |
| Currently focused conversation ID | `focusedConversationAtom` (Jotai) |
| Form input before submit | `useState` |
| Modal open/closed transient | `useState` in modal trigger component |
| Validation errors on a form | `useState` |
| Hover / focus visual state | CSS `:hover`, `:focus`, no React state |

If your case doesn't fit, **ask before inventing a new pattern**. The
matrix is mutually exclusive on purpose.

---

## Git / commit conventions (inherits from project)

* lowercase commit description, no trailing period
* types: `feat / fix / refactor / docs / test / chore / perf / ci`
* scope optional: `feat(web2): ...` allowed but not required
* one logical change per commit
* test + build green before pushing

---

## When a rule conflicts with shipping

Rules in PERFORMANCE.md and this file are **hard**. If a rule blocks
a feature, the answer is not to ship a violation. The answer is to
either:

1. Find a different implementation that doesn't need the violation
2. Open a PR amending the rule (with measurement + sign-off) before
   the feature PR

Shipping a violation "temporarily" is how the legacy `web/` became
a 25665-LOC mess with 3 god files. Don't.

---

## What this doc doesn't cover (deliberately)

* Design system / visual language — separate doc (`DESIGN.md` to come
  in Phase 0.5)
* Accessibility — separate doc (a11y is hard and deserves dedicated
  attention; baseline: every interactive element is keyboard
  reachable, semantic HTML, ARIA labels on icon-only buttons)
* i18n — not in scope for Phase 0 / 1; strings live inline in zh-CN
  for now; component prop API accepts strings (no hard-coded literals
  in components meant to be reused)
* Error boundaries — Phase 1: one ErrorBoundary per route Suspense,
  fallback shows error + retry button + report-bug link
