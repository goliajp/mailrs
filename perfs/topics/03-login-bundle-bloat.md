# Topic 03: `/login` preloads 875 KB of JS the login form never uses

**Status:** open
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

—

## Verification

—
