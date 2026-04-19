# Performance map — mail.golia.ai (v1.4.20, 2026-04-19)

Numbers are median of 3 cold curl runs from a Tokyo residential network unless noted. Network baseline: DNS≈2 ms, TCP+TLS≈25 ms.

Legend: ✓ healthy   ⚠ flagged → see `topics/NN-*.md`   · informational

```
mail.golia.ai (production, v1.4.20)
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
│  │  └─ POST /api/auth/login          0.5 KB    53 ms  ✓
│  └─ rendered (Lighthouse desktop, no throttle)
│     ├─ score 99 / 100
│     ├─ FCP 412 ms · LCP 850 ms · TBT 0 ms · CLS 0.000
│     ├─ Speed Index 883 ms · TTI 412 ms
│     └─ total transfer 1.98 MB
│
├─ /dashboard  (auth)
│  ├─ api (Promise.all, gated by slowest ≈ 400 ms)
│  │  ├─ GET /api/conversations?limit=200    73.3 KB   404 ms  (TTFB 354)  ⚠ topic-01
│  │  ├─ GET /api/mail/stats                  0.5 KB   202 ms  (TTFB 174)  ⚠ topic-02
│  │  └─ GET /api/mail/folders                0.3 KB    57 ms  (TTFB  31)  ✓
│  └─ navigation (puppeteer, networkidle2)
│     ├─ 36 requests · 1.86 MB transfer · CPU 161 ms
│     └─ FCP 96 ms (post-auth, cached SPA)
│
├─ /mail  (auth, chat list)
│  ├─ api (initial)
│  │  ├─ GET /api/conversations?limit=50      36.1 KB   379 ms  (TTFB 340)  ⚠ topic-01
│  │  ├─ GET /api/conversations/categories     0.4 KB    88 ms  ✓
│  │  └─ GET /api/conversations/action-count   0   B    77 ms  ✓
│  ├─ api (tab switches)
│  │  ├─ ?unread=true                                  236 ms  (TTFB 207)  ⚠ topic-01
│  │  ├─ ?starred=true                                 233 ms  (TTFB 203)  ⚠ topic-01
│  │  └─ ?folder=Sent                                   58 ms  ✓
│  ├─ api (open thread)
│  │  ├─ GET /api/conversations/{id}          50.6 KB   140 ms  ✓
│  │  └─ GET /api/conversations/{id}/reactions  0   B    47 ms  ✓
│  └─ navigation
│     └─ 74 requests · 10.5 MB transfer · CPU 325 ms  ⚠ topic-04 (content-driven)
│
├─ /admin/*  (auth, admin)  — section healthy: every endpoint ≤ 80 ms
│  ├─ /admin                       /api/admin/audit/accounts          38 ms  ✓
│  │                               /api/admin/audit-log?limit=10      38 ms  ✓
│  ├─ /admin/domains               /api/admin/domains                 37 ms  ✓
│  ├─ /admin/accounts              /api/admin/accounts                39 ms  ✓
│  ├─ /admin/aliases               /api/admin/aliases                 43 ms  ✓
│  ├─ /admin/apps                  /api/admin/apps                    38 ms  ✓
│  ├─ /admin/groups                /api/admin/groups                  39 ms  ✓
│  │                               /api/admin/permissions             39 ms  ✓
│  ├─ /admin/email-groups          /api/admin/email-groups            40 ms  ✓
│  ├─ /admin/queues                /api/queue                         77 ms  ✓
│  ├─ /admin/audit-log             /api/admin/audit-log?limit=200     49 ms  ✓
│  └─ /admin/system-config         /api/admin/config/smtp             39 ms  ✓
│                                  /api/health                        39 ms  ✓
│                                  /api/status                        38 ms  ✓
│
└─ /settings  (auth)
   └─ navigation: 18 requests · 1.59 MB transfer · CPU 102 ms · (APIs not yet profiled)
```

## Per-page navigation (puppeteer, networkidle2 + 300 ms settle)

| path | reqs | transfer | CPU |
|---|---:|---:|---:|
| /login | 14 | 1.57 MB | 90 ms |
| /dashboard | 36 | 1.86 MB | 161 ms |
| **/mail** | **74** | **10.5 MB** | **325 ms** |
| /admin | 30 | 1.65 MB | 128 ms |
| /admin/accounts | 62 | 1.65 MB | 131 ms |
| /admin/queues | 18 | 1.68 MB | 112 ms |
| /admin/audit-log | 18 | 1.73 MB | 121 ms |
| /settings | 18 | 1.59 MB | 102 ms |
| (other admin pages) | 18–26 | ~1.65 MB | 109–117 ms |

## Open topics

| # | title | severity | scope |
|---|---|---|---|
| [01](topics/01-conversations-slow.md) | `/api/conversations` 340–400 ms TTFB | high | dashboard + /mail |
| [02](topics/02-mail-stats-slow.md) | `/api/mail/stats` 174 ms for 0.5 KB | medium | dashboard |
| [03](topics/03-login-bundle-bloat.md) | `/login` preloads 875 KB unused JS | medium | login cold-start |
| [04](topics/04-mail-page-weight.md) | /mail navigation 10.5 MB / 74 reqs | low | content-driven |

Add topics by appending a row above and creating `topics/NN-slug.md` from the template.
