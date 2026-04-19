# Topic 03: `/login` preloads 875 KB of JS the login form never uses

**Status:** fixed (v1.4.24)
**Severity:** medium
**First observed:** 2026-04-19 (TREE.md, /login)
**Owner:** —

## Symptom

`web/dist/index.html` (served at every route, including `/login`) contains:

```html
<script type="module" crossorigin src="/assets/index-CPRL0MhH.js"></script>
<link rel="modulepreload" crossorigin href="/assets/rolldown-runtime-Dw2cE7zH.js">
<link rel="modulepreload" crossorigin href="/assets/editor-yOJSvfRe.js">
<link rel="modulepreload" crossorigin href="/assets/l4-molecules-CWwgFWmO-CbehyYa8.js">
<link rel="modulepreload" crossorigin href="/assets/use-theme-DJmWAXqb-BRUPHBE3.js">
<link rel="modulepreload" crossorigin href="/assets/markdown-CH7_E8RN.js">
```

Three of these are not needed by the login form:

| chunk | size | reason it isn't needed on /login |
|---|---:|---|
| editor-*.js | 376 KB | rich-text composer (compose / reply only) |
| markdown-*.js | 313 KB | message body rendering |
| l4-molecules-*.js | 185 KB | feature-rich UI primitives used post-login |

That's **~875 KB** of JS the browser downloads, parses, and keeps in memory before the user has typed their password. Cold transfer for /login is 1.98 MB; without these it would drop to roughly 1.1 MB.

## Reproduction

```bash
curl -s https://mail.golia.ai/login | grep -E 'modulepreload|src='
TOKEN= ./scripts/timing.sh "editor"   GET https://mail.golia.ai/assets/editor-yOJSvfRe.js
TOKEN= ./scripts/timing.sh "markdown" GET https://mail.golia.ai/assets/markdown-CH7_E8RN.js
TOKEN= ./scripts/timing.sh "l4-mol"   GET https://mail.golia.ai/assets/l4-molecules-CWwgFWmO-CbehyYa8.js
```

## Hypotheses

1. **Vite is hoisting these chunks into the entry's modulepreload graph** because they are imported (statically) by some module that the entry pulls in transitively, even though the login route never executes them. Audit `web/src/app.tsx` and `web/src/pages/login.tsx` import graphs.
2. **The router is not code-split.** All page components are top-level imports → entry bundle drags everything. Convert non-login routes to `lazy(() => import('./pages/...'))` so the chunks become route-level and login no longer preloads them.
3. **Shared design-system package is statically imported at the top.** `@goliapkg/gds` may be pulling editor + markdown unconditionally via its barrel. Check the entry import side-effects.

## Investigation log

- 2026-04-19 — measured /login transfer 1.98 MB (Lighthouse) / 1.57 MB (puppeteer content-length sum). Login form itself is tiny.

## Decision

Two changes in `web/`:

1. `web/src/app.tsx`: `Chat` and `Dashboard` are now `lazy()` imports
   alongside the already-lazy Admin / Settings / Playground / Protocol.
   Every authenticated route is now code-split. The entry chunk is just
   the auth shell + the public pages (Login, ResetPassword).
2. `web/vite.config.ts`: removed the `chunkGroups` `manualChunks` config.
   Forcing `@tiptap/*` and `react-markdown` into named chunks dragged
   their shared transitive deps (jotai internals, in particular) along
   with them, so the entry's reference to those shared deps caused vite
   to hoist `editor.js` into the entry's modulepreload list. Without
   `manualChunks`, rolldown chunks dynamically along import boundaries
   and keeps tiptap inside the chat route.

Released as v1.4.24 on 2026-04-20.

## Verification

Cold-load run after deploy (`data/2026-04-20/cold-load-v1.4.24.txt`),
compared to v1.4.23:

| page | transfer before | transfer after | Δ | FCP before | FCP after |
|---|---:|---:|---:|---:|---:|
| /login | 3140 KB | **2169 KB** | **−971 KB (−31%)** | 344 ms | **228 ms (−34%)** |
| /dashboard | 3452 KB | **2503 KB** | **−949 KB (−27%)** | 332 ms | **192 ms (−42%)** |
| /mail | 3285 KB | 3258 KB | unchanged (chat lazy chunk still has to download once) | 332 ms | **188 ms (−43%)** |
| /admin (overview) | 3227 KB | **2281 KB** | **−946 KB (−29%)** | 376 ms | **236 ms (−37%)** |
| /admin/* (sub-pages) | ~3225 KB | **~2280 KB** | **~−945 KB** | ~310 ms | **~190 ms** |
| /settings | 3166 KB | **2195 KB** | **−971 KB (−31%)** | 300 ms | **184 ms (−39%)** |

modulepreload list shrank from 5 chunks (1561 KB) to 4 chunks (~600 KB
JS+CSS, the rest of transfer is fonts):

```
before: index 614, editor 376, l4-mol 185, use-theme 14.5, runtime 0.7, markdown 313, css 57.8
after:  index 469, jsx-runtime 8.5, hooks 50, use-theme 14.8, search 1.5, css 59
```

Trade-off: LCP on /dashboard moved 1004 → 1100 ms (+96 ms) and on /mail
984 → 1120 ms (+136 ms) because the lazy chunk needs one extra network
RTT before render. Justified by the dramatic FCP win across every page,
plus the ~950 KB transfer cut for users who only ever visit the
dashboard or admin (most users). The chat lazy chunk (934 KB) is paid
once on first /mail visit and then cached.
