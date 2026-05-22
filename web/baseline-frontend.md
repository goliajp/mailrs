# Frontend perf baseline (pre-polish)

Captured 2026-05-22 from a clean `bun run build` on macOS (M-series).
Numbers below come from the vite build output and `gzip -c | wc -c`. Reproduce:
`cd web && bun run build && ls -lhS dist/assets/`.

## Bundle headline numbers (gzipped)

| Asset class                            | Raw       | Gzip          |
| -------------------------------------- | --------- | ------------- |
| Entry JS (`index-*.js`)                | 576.70 kB | **159.78 kB** |
| Largest async chunk (`chat-*.js`)      | 956.13 kB | **291.21 kB** |
| Admin chunk (`admin-*.js`)             | 80.53 kB  | 14.48 kB      |
| Settings (`settings-*.js`)             | 29.67 kB  | 6.68 kB       |
| Dashboard (`dashboard-*.js`)           | 31.73 kB  | 9.32 kB       |
| Playground                             | 57.39 kB  | 15.07 kB      |
| Protocol                               | 8.38 kB   | 2.69 kB       |
| Stylesheet (`index-*.css`)             | 128.38 kB | 19.92 kB      |
| Total dist/ (incl. fonts + RobotoFlex) | 5.2 MB    | —             |
| JS chunk count                         | 16        | —             |

`(!)` vite warns that chat-*.js > 500 kB and recommends code-splitting.

## Largest source files

```
1466 src/pages/settings.tsx
1429 src/components/thread-view.tsx
1380 src/components/conversation-list.tsx
 784 src/pages/dashboard.tsx
 652 src/pages/admin-accounts.tsx
 561 src/components/reply-box.tsx
 483 src/components/new-conversation.tsx
 440 src/components/invite-card.tsx
 384 src/components/structured-compose.tsx
 377 src/pages/login.tsx
```

## What lives in the `chat` chunk (the 956 kB problem)

Almost the entire **rich editor + markdown rendering stack** ships in the chat
chunk because both `MessageBubble` (display) and `StructuredCompose / TextBlock /
SignatureBlock` (compose) are eagerly imported by `chat.tsx`. Concretely, chat
pulls:

- `@tiptap/react` + `@tiptap/pm` + `@tiptap/starter-kit` + 9 tiptap extensions
- `lowlight` + `highlight.js` common languages (via `CodeBlockLowlight`)
- `react-markdown` + `remark-gfm` + `remark-breaks` + `rehype-highlight`
- `marked` (from `structured-compose.tsx` and `html-renderer.ts`)
- `dompurify` (used in `thread-view.tsx`)

Greps confirming the chunk contents:

```bash
grep -c "prosemirror\|tiptap\|lowlight\|highlight.js" dist/assets/chat-*.js   # 28
grep -c "react-markdown\|remark-gfm\|rehype-highlight\|marked" dist/assets/chat-*.js  # 7
```

The signature editor (`signature-block.tsx`) is the only tiptap consumer that
remains after `structured-compose` removed its inline rich editor — and it only
mounts when the user has a signature configured. text-block uses
`react-markdown` only when in preview mode, but the import is eager.

## What lives in the `index` (entry) chunk

The entry chunk includes:

- React 19 + react-dom + react-router
- jotai + jotai/utils
- @tanstack/react-query + persist-client + sync-storage-persister
- @goliapkg/gds (AppShell, theme, fonts, toast)
- Everything in `src/components/` and `src/lib/` that isn't gated behind a lazy
  page (sidebar, command palette, error boundary, mobile shell, auth gate,
  health-check effects, status bar, etc.)
- `src/pages/login.tsx` and `src/pages/reset-password.tsx` (eagerly imported in
  `app.tsx` so the login path renders without a chunk fetch)

The reason this is 160 kB gzipped is mainly the four big vendor families
(React + Router, react-query, jotai, gds). The lazy split between authenticated
pages is already in place (see `app.tsx` — `Admin`, `Chat`, `Dashboard`,
`Playground`, `Protocol`, `Settings` are all `lazy()`).

## Suspected render hotspots

After reading the five largest .tsx files:

1. `src/components/conversation-list.tsx` — ALREADY well-optimized:
   `ConversationItem` is `memo()`'d, sortedConversations is `useMemo()`, all
   callbacks are `useCallback()`, list is virtualized with
   `@tanstack/react-virtual` (`useVirtualizer`).
2. `src/components/thread-view.tsx` — ALREADY well-optimized: uses `memo` on
   sub-components, uses `selectAtom` for primitive subscriptions (intentional
   defense against the WS-driven array-reref re-render), uses
   `useDeferredValue` in `MessageBubble` for heavy `splitEmail` work.
3. `src/pages/dashboard.tsx` — uses `useQueries` + `useMemo` + `useCallback`
   appropriately. The 200-conversation list it pulls is rendered inline (no
   virtualization), but since this is a dashboard-style "show recent 5–10
   pinned/unread" view it slices to small sublists.
4. `src/pages/settings.tsx` — 1466 lines of admin form UI. Most state is local
   `useState`; no obvious render-loop bugs. Conditional render by `category`
   means only one panel mounts at a time.
5. `src/components/reply-box.tsx` — straightforward; the heavy compose UI is
   in the `StructuredCompose` child which uses uncontrolled imperative refs to
   avoid re-rendering the entire reply tree on each keystroke.

No glaring "inline `() => ...` in props on a memo'd child" anti-patterns.
The existing code already shows previous perf work — the comments call out
several specific decisions (selectAtom for primitive subscription, group-hover
class instead of useState, EMPTY_ATTACHMENTS sentinel for memo stability).

## Other observations

- `src/pages/admin.tsx` (the lazy `Admin` parent) imports ALL 11 admin sub-pages
  eagerly. That bundles the entire admin surface into the admin chunk
  (80 kB), which is fine — admin is rarely visited and chunk-level caching
  amortizes it — but per-tab lazy split would let an admin landing on
  `/admin/overview` skip the bytes for `system-config`, `mail-audit`, etc.
- The chat chunk's tiptap + markdown payload is the single biggest target.
  Even a partial split (markdown viewer in chat chunk, tiptap-based signature
  editor lazy on settings open) would meaningfully shrink the chat chunk.
- vite.config.ts already documents (lines 49-51) that a previous attempt at
  manualChunks made things worse by leaking jotai into the tiptap group.
  We need to be careful here.
- The font assets dominate the dist/ total (RobotoFlex.ttf alone is 1.79 MB,
  Inter weights ~140-150 kB each, JetBrainsMono ~95 kB each). These are loaded
  by gds; not in scope for this polish unless gds exposes a way to subset.

## Tests baseline

`bun run test` → 25 files, **451 passed**, 0 failed, 0 skipped, 2.24s.

## Build time baseline

`bun run build` → vite finishes in **318ms** after tsc + eslint + prettier
(roughly 5-7s total for those checks).

## Polish targets (in priority order)

1. Move `lowlight` + `highlight.js` out of the eager chat chunk path. The
   `CodeBlockLowlight` extension is only needed when the user actually opens
   the rich editor inside the signature block, AND the rich editor is now
   only used for signatures (per `structured-compose.tsx` comment "rich is
   gone, markdown is the only authoring mode"). Lazy-import the signature
   block.
2. Lazy-import the markdown preview pipeline from `text-block.tsx` —
   `react-markdown` + remark/rehype only needed in preview mode.
3. Lazy-import `marked` in `structured-compose.tsx` — it's only used in the
   "convert HTML to markdown when paste/replace happens" path.
4. Lazy each admin sub-page in `admin.tsx`.
5. Define a small manualChunks config that splits the four big vendor families
   into their own cacheable chunks WITHOUT producing the jotai-leak the
   existing comment warns about. Validate by examining the resulting chunk
   graph.
6. Move `dompurify` to lazy or defer where possible (only invoked on rare
   "show source" actions in thread-view, per code).
