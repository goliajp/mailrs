# Performance map — mail.golia.ai (v1.4.23, 2026-04-20)

Numbers are median of 3 cold curl runs from a Tokyo residential network unless noted. Network baseline: DNS≈2 ms, TCP+TLS≈25 ms.
Cold-load page metrics (FCP/LCP/CLS) come from `scripts/cold-load.js` — fresh browser context per page, cache disabled, PerformanceObserver instrumented.

Legend: ✓ healthy   ⚠ flagged → see `topics/NN-*.md`   · informational   ⏱ measured (no concern)

```
mail.golia.ai (production, v1.4.21)
│
├─ /login  (public)
│  ├─ assets
│  │  ├─ HTML                          1.2 KB    38 ms  ✓
│  │  ├─ index-*.js (entry)          614   KB   123 ms  ·
│  │  ├─ editor-*.js                 376   KB   116 ms  ⚠ topic-03 (preloaded, unused on /login)
│  │  ├─ markdown-*.js               313   KB   106 ms  ⚠ topic-03 (preloaded, unused on /login)
│  │  ├─ l4-molecules-*.js           185   KB    90 ms  ⚠ topic-03 (preloaded, unused on /login)
│  │  ├─ use-theme-*.js               14.5 KB    55 ms  ·
│  │  ├─ rolldown-runtime-*.js         0.7 KB    46 ms  ·
│  │  └─ index-*.css                  57.8 KB    75 ms  ·
│  ├─ api
│  │  └─ POST /api/auth/login          0.5 KB    41 ms  ✓
│  └─ rendered
│     ├─ Lighthouse score 99 / 100 (no throttle, desktop)
│     ├─ FCP 340 ms · LCP 368 ms · CLS 0.000 · idle 993 ms
│     └─ total transfer 3.14 MB (cold)
│
├─ /dashboard  (auth)
│  ├─ api (Promise.all, gated by slowest ≈ 320 ms after v1.4.21)
│  │  ├─ GET /api/conversations?limit=200    73.4 KB   323 ms  (TTFB 271)  ⚠ topic-01 (was 354 → 271)
│  │  ├─ GET /api/mail/stats                  0.5 KB   202 ms  (TTFB 175)  ⚠ topic-02
│  │  └─ GET /api/mail/folders                0.3 KB    56 ms  (TTFB  31)  ✓
│  └─ rendered
│     ├─ FCP 332 ms · LCP 1004 ms · idle 2006 ms
│     └─ CLS 0.002 ✓ (was 0.443 before topic-05 fix in v1.4.23)
│
├─ /mail  (auth, chat list)
│  ├─ api (initial)
│  │  ├─ GET /api/conversations?limit=50      36.1 KB   308 ms  (TTFB 270)  ⚠ topic-01 (was 379 → 308)
│  │  ├─ GET /api/conversations/categories     0.4 KB    89 ms  ✓
│  │  └─ GET /api/conversations/action-count   0   B    79 ms  ✓
│  ├─ api (tab / section / category switches)
│  │  ├─ ?unread=true                                   258 ms  (TTFB 231)  ⚠ topic-01 (residual)
│  │  ├─ ?starred=true                                  254 ms  (TTFB 227)  ⚠ topic-01 (residual)
│  │  ├─ ?folder=Sent                                    62 ms  ✓
│  │  ├─ ?category=spam                                 116 ms                ✓
│  │  ├─ ?section=action                                313 ms  (TTFB 269)  · (peer, unchanged by fix)
│  │  ├─ ?section=important                             304 ms  (TTFB 266)  ✓ topic-07 fixed v1.4.22 (was 581/376)
│  │  └─ ?section=other                                 297 ms  (TTFB 257)  ✓ topic-07 fixed v1.4.22
│  ├─ api (open thread)
│  │  ├─ GET /api/conversations/{id}          46.0 KB   138 ms  ✓
│  │  └─ GET /api/conversations/{id}/reactions  0   B    37 ms  ✓
│  ├─ api (search)
│  │  ├─ GET /api/conversations/search?q=invoice  25.3 KB   634 ms  (TTFB 596)  ⚠ topic-06
│  │  └─ GET /api/conversations/search?q=金额    11.0 KB   612 ms  (TTFB 576)  ⚠ topic-06 (CJK ILIKE)
│  └─ rendered
│     ├─ FCP 476 ms · LCP 1140 ms · idle 1850 ms                              ⚠ topic-04 (page weight)
│     └─ CLS 0.021 ✓ · CPU 387 ms · 93 reqs / 3.46 MB (cold cache)
│
├─ /admin/*  (auth, admin)  — every list endpoint ≤ 115 ms total
│  ├─ /admin   (overview)           /api/admin/audit/accounts       40 ms  ✓
│  │                                /api/admin/audit-log?limit=10   38 ms  ✓
│  │                                CLS 0.000 ✓ (was 0.223 before topic-05 fix in v1.4.23)
│  ├─ /admin/domains                /api/admin/domains              37 ms  ✓
│  ├─ /admin/accounts               /api/admin/accounts             45 ms  ✓
│  ├─ /admin/aliases                /api/admin/aliases              38 ms  ✓
│  ├─ /admin/apps                   /api/admin/apps                 39 ms  ✓
│  ├─ /admin/groups                 /api/admin/groups               47 ms  ✓
│  │                                /api/admin/permissions         114 ms  · (one outlier — likely network jitter, payload 0.2 KB)
│  ├─ /admin/email-groups           /api/admin/email-groups         67 ms  ✓
│  ├─ /admin/queues                 /api/queue                     115 ms  · (largest admin payload 8.4 KB)
│  ├─ /admin/audit-log              /api/admin/audit-log?limit=200  98 ms  ✓
│  ├─ /admin/system-config          /api/admin/config/smtp          75 ms  ✓
│  │                                /api/health                     36 ms  ✓
│  │                                /api/status                     40 ms  ✓
│  └─ rendered (sub-pages)
│     └─ FCP 290–420 ms · LCP 600–740 ms · CLS ≤ 0.015 ✓
│
├─ /settings  (auth)  — all GETs ≤ 42 ms total ✓
│  ├─ GET /api/auth/recovery-email                          39 ms  ✓
│  ├─ GET /api/auth/totp/status                             37 ms  ✓
│  ├─ GET /api/mail/keys/status                             37 ms  · returns 400 (PGP not configured for this account)
│  ├─ GET /api/mail/signatures                              42 ms  ✓
│  ├─ GET /api/agent/keys                                   41 ms  ✓
│  └─ GET /api/agent/webhooks                               39 ms  ✓
│
└─ misc surfaces
   ├─ GET /api/contacts?q=&limit=20                         80 ms  ✓
   ├─ GET /api/bimi/golia.jp                                37 ms  ✓
   └─ POST /api/conversations/batch                         39 ms  ✓ (request-shape only)
```

## Per-page cold load (cold-load.js, networkidle2 + 400 ms settle)

| path | TTFB | FCP | LCP | idle | reqs | transfer | CLS | CPU |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| /login | 36 | 340 | 368 | 993 | 12 | 3.14 MB | 0.000 ✓ | 155 |
| /dashboard | 38 | 316 | **892** | 1516 | 34 | 3.45 MB | **0.443** ⚠ | 180 |
| **/mail** | 35 | 476 | **1140** | **1850** | **93** | **3.46 MB** | 0.021 ✓ | **387** |
| /admin | 37 | 300 | 696 | 1266 | 30 | 3.23 MB | **0.223** ⚠ | 166 |
| /admin/domains | 38 | 304 | 600 | 1267 | 17 | 3.22 MB | 0.001 ✓ | 147 |
| /admin/accounts | 54 | 404 | 704 | 1369 | 61 | 3.23 MB | 0.015 ✓ | 168 |
| /admin/aliases | 68 | 416 | 736 | 1379 | 21 | 3.22 MB | 0.003 ✓ | 145 |
| /admin/apps | 49 | 388 | 708 | 1353 | 21 | 3.22 MB | 0.003 ✓ | 146 |
| /admin/groups | 38 | 300 | 612 | 1278 | 25 | 3.22 MB | 0.004 ✓ | 143 |
| /admin/email-groups | 35 | 296 | 608 | 1260 | 21 | 3.22 MB | 0.001 ✓ | 138 |
| /admin/queues | 40 | 288 | 672 | 1254 | 17 | 3.25 MB | 0.006 ✓ | 148 |
| /admin/audit-log | 40 | 304 | 640 | 1268 | 17 | 3.31 MB | 0.001 ✓ | 163 |
| /admin/system-config | 38 | 308 | 632 | 1273 | 17 | 3.22 MB | 0.000 ✓ | 183 |
| /settings | 36 | 296 | 624 | 1264 | 17 | 3.17 MB | 0.000 ✓ | 146 |

(times in ms)

## Open topics

| # | title | severity | scope |
|---|---|---|---|
| [01](topics/01-conversations-slow.md) | `/api/conversations` TTFB residual 270–280 ms | medium | dashboard + /mail; partially fixed v1.4.21 |
| [02](topics/02-mail-stats-slow.md) | `/api/mail/stats` 174 ms for 0.5 KB | medium | dashboard |
| [03](topics/03-login-bundle-bloat.md) | `/login` preloads 875 KB unused JS | medium | login cold-start |
| [04](topics/04-mail-page-weight.md) | /mail LCP 1140 ms / 10 MB / 93 reqs | low | content-driven |
| ~~[05](topics/05-cls-dashboard-admin.md)~~ | dashboard CLS 0.443→0.002, admin 0.223→0.000 | resolved | fixed in v1.4.23 |
| [06](topics/06-search-conversations-slow.md) | `/api/conversations/search` 596 ms TTFB | high | every keystroke in search bar |
| ~~[07](topics/07-section-important-slow.md)~~ | `?section=important` 581→304 ms (-48%) | resolved | fixed in v1.4.22 |

Add topics by appending a row above and creating `topics/NN-slug.md` from the template.
