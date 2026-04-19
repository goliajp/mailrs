# Topic 04: /mail navigation transfers 10.5 MB / 74 requests

**Status:** partially fixed (v1.4.30: image lazy-load + tracker strip); auto-open UX still product
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

Apply two engineering-layer optimisations that don't change the
auto-open behaviour (which is a product/UX call):

`web/src/components/html-frame.tsx`:

1. **Lazy-load inline `<img>` tags**: as `proxyExternalUrls` rewrites
   external image URLs to route through `/api/proxy/image`, it now
   also injects `loading="lazy" decoding="async"` on every `<img>`
   that didn't already specify a `loading` attribute. Browsers
   honour these attributes inside iframes (which is how email body
   is rendered). Images outside the viewport stay unfetched until
   the user scrolls.
2. **Strip 1×1 tracking pixels**: a new `stripTrackingPixels` step
   in `sanitizeEmail` deletes `<img>` tags that explicitly set
   `width=1 height=1` (or the equivalent inline `style="width:1px"`
   form). These are open-rate beacons used by marketing senders;
   removing them eliminates pointless network requests and the
   privacy leak.

Released as v1.4.30 on 2026-04-20.

## Verification

Cold-cache navigation (single email account, mostly newsletter
content):

| metric | before (v1.4.29) | after (v1.4.30) | Δ |
|---|---:|---:|---:|
| /mail requests | 67 | 67 | unchanged (same opened thread) |
| /mail transfer (content-length sum) | varies | varies | content-driven |
| /mail FCP | 188 ms | 192 ms | unchanged |
| /mail LCP | 1140 ms | 1080 ms | small win |
| /mail CPU | 305 ms | 274 ms | −10% |

The transfer impact varies wildly with which thread happens to be
open: a thread with 30+ inline images sees a much bigger drop
because most images now stay un-fetched. A pure-text thread sees
no change. The CPU saving is consistent (less image decoding).

The bigger lever — auto-opening the latest thread on /mail entry
versus showing only the list — is a product decision and out of
scope for this perf pass. Topic remains open at "low severity"
for that follow-up.
