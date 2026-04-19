# Topic 05: /dashboard CLS 0.443 + /admin CLS 0.223 (Web Vitals "poor")

**Status:** fixed (v1.4.23)
**Severity:** high (user-perceived layout jank)
**First observed:** 2026-04-19 (data/2026-04-19/cold-load.txt)
**Owner:** —

## Symptom

Cold-load PerformanceObserver readings (cache disabled, fresh browser context):

| page | CLS | Web Vitals band |
|---|---:|---|
| /dashboard | **0.443** | **poor** (>0.25) |
| /admin (overview) | **0.223** | needs improvement (>0.1) |
| /mail | 0.021 | good |
| /admin/* (sub-pages) | 0.001–0.015 | good |
| /login | 0.000 | good |

CLS measures how much content shifts after the first paint. 0.443 means the page repositions roughly half a viewport's worth of pixels after the user already started looking. /dashboard and /admin overview are the only routes whose value lands in the "poor" band.

## Reproduction

```bash
TOKEN=… bun perfs/scripts/cold-load.js
```

CLS column captures cumulative layout shift up to 400 ms after `networkidle2`.

## Hypotheses

1. **/dashboard renders sections progressively as `Promise.all([conversations, stats, folders])` resolves.** The page reserves no space for stat cards or recent-unread rows, so when the data arrives every section pushes everything else down. `web/src/pages/dashboard.tsx` shows a single skeleton during `loading`, then swaps to the populated layout in one render — but the populated layout's row count is data-dependent.
2. **Stat cards' values change "0 → real number"** mid-render: the digits widen and the cards reflow. Adding fixed minimum widths to the value digit area should pin them.
3. **/admin overview likely has the same pattern** — sections stacking as audit-log + audit/accounts API responses arrive.
4. **No `aspect-ratio` / explicit height on async-loaded images** (avatars, BIMI logos in conversation rows). Each one snaps from 0×0 → 28×28 when it loads, shifting siblings.

## Investigation log

- 2026-04-19 — discovered when `cold-load.js` was extended with PerformanceObserver. Prior `page-perf.js` runs did not capture CLS.

## Decision

Stop swapping the DOM tree on data arrival. Render the loaded layout's
shape always, replace data-driven content with same-sized placeholders
during loading.

`web/src/pages/dashboard.tsx`:
- removed the early-return skeleton branch
- `StatCard` accepts `loading`; renders a fixed-width placeholder bar
  in the value column (so digit width 0→N doesn't push the card)
- left + right columns render `SectionSkeleton` placeholders during
  loading; the rest of the layout structure is identical to loaded

`web/src/pages/admin-overview.tsx`:
- `StatusBanner` wrapped in a `min-h-[60px]` container with a
  `BannerSkeleton` fallback
- `MetricCard` accepts `loading`, fixed-height value row, sub line
  always rendered (with empty string) for stable height
- Services section reserves `min-h-[88px]` and renders pill-shaped
  placeholders before /api/health resolves
- `SmtpConfigPanel` falls back to `PanelSkeleton` until /api/admin/config/smtp

Released as v1.4.23 on 2026-04-20.

## Verification

Cold-load run after deploy (`data/2026-04-20/cold-load-v1.4.23.txt`):

| page | CLS before (v1.4.22) | CLS after (v1.4.23) | Web Vitals band |
|---|---:|---:|---|
| /dashboard | 0.443 | **0.002** | ✓ good (was poor) |
| /admin (overview) | 0.223 | **0.000** | ✓ good (was needs improvement) |
| /mail | 0.021 | 0.010 | ✓ good (unchanged) |
| /admin/* sub-pages | ≤ 0.015 | ≤ 0.015 | ✓ good (unchanged) |
| /login | 0.000 | 0.000 | ✓ good |

Trade-offs: /dashboard idle moved 1516 → 2006 ms, LCP 892 → 1004 ms
because the placeholder skeleton is rendered before data arrives,
giving the browser more painting work up front. The user perceives
LCP roughly the same (still under 1.1 s, "needs improvement" band)
but no longer experiences the layout jump — that's the tradeoff
B1+B7 was meant to make.
