# Performance audit — 2026-04-19

Target: `https://mail.golia.ai` (production, v1.4.20). Account `lihao@golia.jp`.
Probed from a residential network in Tokyo (DNS≈2 ms, TCP+TLS≈25 ms baseline).

---

## 1. /login — login screen

### Real page load (Lighthouse, desktop, no throttling)
| metric | value |
|---|---|
| Performance score | **99 / 100** |
| FCP | 412 ms |
| LCP | 850 ms |
| TBT | 0 ms |
| CLS | 0.000 |
| Speed Index | 883 ms |
| TTI | 412 ms |
| Total transfer | **1.98 MB** |

### Static asset breakdown (3 cold curl runs, median)
| asset | size | total |
|---|---|---|
| /login HTML | 1.2 KB | 38 ms |
| /assets/index-*.js (entry) | **614 KB** | 123 ms |
| /assets/editor-*.js | **376 KB** | 116 ms |
| /assets/markdown-*.js | **313 KB** | 106 ms |
| /assets/l4-molecules-*.js | 185 KB | 90 ms |
| /assets/use-theme-*.js | 14.5 KB | 55 ms |
| /assets/rolldown-runtime-*.js | 0.7 KB | 46 ms |
| /assets/index-*.css | 57.8 KB | 75 ms |
| POST /api/auth/login | 0.5 KB | 53 ms |

**Issue:** `editor`, `markdown`, `l4-molecules` are `<link rel="modulepreload">`-ed from `index.html` and downloaded on `/login` even though the login form needs none of them. 875 KB of JS is fetched eagerly that the user only needs after authentication.

---

## 2. /dashboard — first page after login

API calls fired in parallel by `pages/dashboard.tsx`:

| endpoint | size | TTFB | total |
|---|---|---|---|
| GET /api/conversations?limit=200 | 73.3 KB | **354 ms** | 404 ms |
| GET /api/mail/stats | 0.5 KB | **174 ms** | 202 ms |
| GET /api/mail/folders | 0.3 KB | 31 ms | 57 ms |

Wall time gated by the slowest of the three ≈ **400 ms** server-side, before any rendering.

**Issue:** `/api/conversations?limit=200` is the dashboard's slowest request and ships 73 KB just so the dashboard can compute "today / pinned / needs-attention / recent unread / top senders" client-side. The dashboard already has `/api/mail/stats` for unread counts — most of the other derived numbers could be a single small `/api/dashboard` aggregate.

---

## 3. /mail — chat list (default view)

| endpoint | size | TTFB | total |
|---|---|---|---|
| GET /api/conversations?limit=50 | 36.1 KB | **340 ms** | 379 ms |
| GET /api/conversations/categories | 0.4 KB | 63 ms | 88 ms |
| GET /api/conversations/action-count | 0.0 KB | 51 ms | 77 ms |

Tab switches re-fetch:

| endpoint | TTFB | total |
|---|---|---|
| `&unread=true` | 207 ms | 236 ms |
| `&starred=true` | 203 ms | 233 ms |
| `&folder=Sent` | 21 ms | 58 ms |

**Observation:** the unfiltered `/api/conversations?limit=50` is ~140 ms slower than the filtered variants and ~60 ms slower than `?limit=200` from the same endpoint earlier (consistent across runs). That points to per-thread enrichment work (ranking, dedup, importance, last-message snippet) that is not bounded by the page-size parameter.

Real navigation (puppeteer): /mail page transfers **10.5 MB / 74 requests** because each opened thread streams attachments and rendered HTML. The 10 MB is real content, not bundle bloat.

---

## 4. Open a thread

| endpoint | size | TTFB | total |
|---|---|---|---|
| GET /api/conversations/{id} | 50.6 KB | 90 ms | 140 ms |
| GET /api/conversations/{id}/reactions | 0.0 KB | 20 ms | 47 ms |

This part is healthy.

---

## 5. Admin pages

All admin endpoints checked — every one is ≤ 80 ms total:

| page | endpoint | total |
|---|---|---|
| admin-overview | /api/admin/audit/accounts | 38 ms |
| admin-overview | /api/admin/audit-log?limit=10 | 38 ms |
| admin-domains | /api/admin/domains | 37 ms |
| admin-accounts | /api/admin/accounts | 39 ms |
| admin-aliases | /api/admin/aliases | 43 ms |
| admin-apps | /api/admin/apps | 38 ms |
| admin-groups | /api/admin/groups | 39 ms |
| admin-groups | /api/admin/permissions | 39 ms |
| admin-email-groups | /api/admin/email-groups | 40 ms |
| admin-queues | /api/queue | 77 ms |
| admin-audit-log | /api/admin/audit-log?limit=200 | 49 ms |
| admin-system-config | /api/admin/config/smtp | 39 ms |
| admin-system | /api/health | 39 ms |
| admin-system | /api/status | 38 ms |

Admin section is not a backend bottleneck.

---

## 6. SPA navigation (puppeteer, networkidle2 + 300 ms settle)

| path | reqs | transfer | CPU task time |
|---|---|---|---|
| /login | 14 | 1.57 MB | 90 ms |
| /dashboard | 36 | 1.86 MB | 161 ms |
| **/mail** | **74** | **10.5 MB** | **325 ms** |
| /admin (overview) | 30 | 1.65 MB | 128 ms |
| /admin/accounts | 62 | 1.65 MB | 131 ms |
| /admin/queues | 18 | 1.68 MB | 112 ms |
| /admin/audit-log | 18 | 1.73 MB | 121 ms |
| /settings | 18 | 1.59 MB | 102 ms |
| (other admin pages) | 18–26 | ~1.65 MB | 109–117 ms |

Every authenticated page transfers ≥ 1.6 MB even when it doesn't need the editor or markdown chunks — caching helps after the first visit, but cold-start is heavy.

---

## Findings, ranked

1. **`/api/conversations` is the dominant page-load latency on dashboard and /mail.**
   340–400 ms TTFB for what is largely a paginated list query. Every unfiltered fetch pays this cost, and `/mail` does it twice in some flows (initial load + tab switch). Worth investigating: which SQL plan, are there N+1 enrichments per thread, can the per-thread metadata be precomputed and cached. (Reminder: data layer should hold both facts and derivations — looks like a derivation/snapshot is missing here.)
2. **`/api/mail/stats` 174 ms is disproportionate** for a tiny 0.5 KB JSON. Likely a `COUNT(*)` over messages with no covering index.
3. **`/login` eagerly preloads `editor.js` (376 KB) + `markdown.js` (313 KB) + `l4-molecules.js` (185 KB).** The login form needs none of them. Splitting the entry so `/login` only pulls auth-related code would cut cold transfer by ~875 KB.
4. **Admin section is fine** — every endpoint < 80 ms, every page renders well under 200 ms client side.
5. **/mail page weight (10 MB)** is content, not bloat. Improvement vector is lazy-loading message bodies / images on demand rather than eagerly fetching all visible threads' content.

---

## Reproduce

- Asset / API timing: `cd perfs && TOKEN=… ./timing.sh "<label>" GET <url>`
- Per-page navigation: `bun perfs/page-perf.js`
- Login Lighthouse: `bunx --bun lighthouse https://mail.golia.ai/login --output=json --output-path=perfs/login.lh.json --chrome-flags="--headless=new" --form-factor=desktop --throttling-method=provided --screenEmulation.disabled`
