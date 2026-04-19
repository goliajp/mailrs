# Topic 04: /mail navigation transfers 10.5 MB / 74 requests

**Status:** open (likely accepted)
**Severity:** low
**First observed:** 2026-04-19 (TREE.md, /mail)
**Owner:** —

## Symptom

Puppeteer navigation to `/mail` (networkidle2 + 300 ms settle) records:

- **74 requests**
- **10.5 MB transfer**
- 325 ms CPU task time

Other authenticated pages cluster around 18–62 requests and 1.6–1.9 MB. The /mail outlier is real email content — body HTML, inline images, tracking pixels, attachment thumbnails — for the auto-selected thread plus warm previews.

## Reproduction

```bash
bun perfs/scripts/page-perf.js
```

## Hypotheses

1. **Default-thread auto-open eagerly fetches body + all attachments.** chat.tsx auto-selects conversations[0]; ThreadView loads full message bodies and all attachments immediately. For an account with image-heavy newsletters this is 5–10 MB by itself. Lazy-loading attachments / images on intersection observer would cut weight without sacrificing UX.
2. **Inline images are proxied at full resolution.** Check whether `/api/messages/{uid}/attachments/{n}` serves the original or a thumbnail.
3. **The list also prefetches snippets/avatars.** SenderAvatar may issue per-sender requests.

## Investigation log

- 2026-04-19 — measured. Likely a UX choice rather than a bug — needs a product call before we change behavior.

## Decision

—

## Verification

—
