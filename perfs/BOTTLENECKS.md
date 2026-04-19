# Bottlenecks — debug worksheet (v1.4.23, 2026-04-20)

A flat, opinionated punch list. Read top-to-bottom — items at the top hurt the most users per hour. Each row links to a topic file with reproduction, root-cause analysis, fix candidates and (when done) verification.

For the system-wide picture see `TREE.md`. For the workflow rules see `README.md`.

---

## Tier 1 — fix soon

These are user-perceived on every visit and have a known low-risk fix.

### ~~B1 · /dashboard CLS 0.443~~ → topic-05 → **fixed in v1.4.23**
~~Web Vitals "poor" band (>0.25). Whole stat-card row + recent-unread list jump as `Promise.all` resolves. Fix is purely frontend: reserve fixed dimensions for skeleton placeholders, pin stat card width, set explicit `aspect-ratio` on avatars.~~
**Result:** CLS 0.443 → 0.002 (Web Vitals "good"). Achieved by removing the binary skeleton↔layout swap; layout structure is now identical loading vs loaded, only inner content swaps. Dashboard idle paid +500 ms (skeleton renders earlier) — accepted trade-off vs. the layout jump.

### B2 · `/api/conversations/search` 596 ms TTFB  → topic-06
Every keystroke in the search bar pays this. Root cause: `OR` chain of 5 ILIKE columns + EXISTS over `attachment_content` defeats index combination, plus the same per-thread correlated subqueries from list_conversations. Fix-a: gate ILIKE on tsvector miss only (CTE).
**impact:** every search query · **fix size:** medium SQL change · **risk:** low (semantics preserved for tsvector hits).

### ~~B3 · `?section=important` 581 ms total~~ → topic-07 → **fixed in v1.4.22**
~~HAVING-clause SubPlan (per-group `LIMIT 1` over `messages` ordered by `importance_score`) — same pattern that topic-01 fix-a already proved fixable. Replace with ordered aggregate.~~
**Result:** 581 → 304 ms total (−48%); 376 → 266 ms TTFB (−29%). EXPLAIN: 352 → 208 ms (−41%). `section=other` came along for free.

---

## Tier 2 — measurable, but each visit pays it once

### B4 · `/api/conversations` residual 270–280 ms TTFB  → topic-01 (partially fixed v1.4.21)
fix-a deployed. Residual cost lives in SubPlan 5 (requires_action per row, 17 280 loops) + SubPlan 8 (NOT EXISTS spam/scam per row, 18 762 loops) + the external-merge sort to disk. fix-c (LATERAL email_analysis) and fix-b (`work_mem` to 16 MB) are the next levers; EXPLAIN predicts another ~60 ms.
**impact:** dashboard + /mail + every tab switch · **fix size:** medium SQL + low-risk config tweak · **risk:** low.

### B5 · `/api/mail/stats` 174 ms TTFB / 0.5 KB  → topic-02
Tiny payload, big TTFB → server-side computation. Almost certainly an unindexed `COUNT(*)` over `messages` for total/unread, plus a maildir walk for `storage_bytes`. EXPLAIN not yet captured.
**impact:** every dashboard load (Promise.all gate) · **fix size:** depends on EXPLAIN — likely indexes · **risk:** low.

### B6 · `/login` preloads 875 KB unused JS  → topic-03
`<link rel=modulepreload>` of `editor.js` (376 KB), `markdown.js` (313 KB), `l4-molecules.js` (185 KB). The login form needs none of them. Splitting the entry so unauthenticated routes only pull auth code drops cold transfer from 1.98 MB to ~1.1 MB.
**impact:** every cold-cache visit · **fix size:** vite config + route-level lazy() · **risk:** low if done route by route.

### ~~B7 · /admin overview CLS 0.223~~ → topic-05 → **fixed in v1.4.23**
~~Same pattern as B1, smaller magnitude (Web Vitals "needs improvement"). The fix for B1 likely covers this too if applied as a generic "reserve space for async sections" pattern.~~
**Result:** CLS 0.223 → 0.000 (Web Vitals "good"). Same fix shape as B1 — wrap each conditional `{health && …}` block in a `min-h-` container with a structurally-matched skeleton.

---

## Tier 3 — known, accepted unless something changes

### B8 · /mail LCP 1140 ms / idle 1850 ms / 93 reqs / 10 MB  → topic-04
Real email content. Auto-opens the latest thread, fetches every attachment + image. Lazy-loading attachments + images on intersection observer would cut this dramatically; needs a product call before changing the auto-open UX.
**impact:** every /mail visit · **fix size:** medium frontend (lazy loading) + product decision · **risk:** changes UX.

---

## What is healthy (no action)

- All `/api/admin/*` endpoints ≤ 115 ms.
- All `/settings` endpoints ≤ 42 ms.
- `/api/conversations/{id}` (open thread) 138 ms.
- `?folder=Sent`, `?category=spam`, `/api/contacts`, `/api/bimi/*` all sub-120 ms.
- /login Lighthouse score 99/100, FCP 412 ms, LCP 850 ms (real first-paint).
- All non-flagged pages CLS ≤ 0.025.

---

## Suggested order

Remaining order after v1.4.23 (2026-04-20):

1. ~~**B3**~~ done v1.4.22 (?section=important 581 → 304 ms).
2. ~~**B1 + B7**~~ done v1.4.23 (CLS 0.443/0.223 → 0.002/0.000).
3. **B4** (continue topic-01 with fix-c LATERAL JOIN; pair with `work_mem` bump).
4. **B6** (route-level code splitting; reduces cold-start for everyone).
5. **B5** (depends on EXPLAIN).
6. **B2** (search rewrite).
7. **B8** (needs product input).

---

## Reproduce all numbers

```bash
cd perfs
TOKEN=$(curl -s -X POST https://mail.golia.ai/api/auth/login \
   -H 'Content-Type: application/json' \
   -d '{"address":"…","password":"…"}' | jq -r .token) && export TOKEN

./scripts/sweep-apis.sh   > data/$(date +%F)/sweep.txt          # all API timings
bun  scripts/cold-load.js > data/$(date +%F)/cold-load.txt      # per-page FCP/LCP/CLS
```
