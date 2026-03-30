# Classic Errors

Mistakes that have been made before. Do not repeat them.

## Stale async cache after mutation

**Context:** Any system where a background worker produces cached results (syntax highlighting, search indexing, LSP diagnostics) while the main thread mutates the source data.

**Bug:** After an edit, the async worker may return results computed from the PRE-edit source. These results contain byte/line offsets that no longer match the new content. If used directly, they cause silent rendering failures (text not drawn, garbled layout, phantom content).

**Root cause chain:**
1. User edits text → source data changes
2. Old async request (submitted before the edit) completes and returns stale offsets
3. Main thread accepts the stale result and overwrites any invalidation you did
4. Renderer uses stale byte ranges → `span_end > line_text.len()` → span skipped → nothing drawn

**Fix — two layers required:**
1. **Immediate invalidation:** When source data is mutated, immediately truncate/clear the cache from the mutation point onward. This ensures the current frame renders with a safe fallback (e.g., plain text without syntax colors).
2. **Stale result rejection:** When polling async results, check whether the source was modified since the request was submitted. If so, discard the result — do not let it overwrite the invalidated cache.

**General rule:** Whenever an async producer and a synchronous mutator share a cache, the mutator must both (a) invalidate the cache immediately and (b) ensure no in-flight async result can overwrite that invalidation. Generation counters or dirty flags work for (b).

## macOS Metal live resize wobble

**Context:** Any Metal/CAMetalLayer app on macOS where the window is resizable.

**Bug:** During live window resize, content visibly stretches/wobbles because the compositor scales the old drawable to fill the new window size before the app renders a new frame at the correct size.

**Root cause chain:**
1. User drags window edge → macOS resizes the window continuously
2. CAMetalLayer's drawable size lags behind the actual window size
3. Compositor stretches the old frame to fit the new bounds (default `contentsGravity = resize`)
4. App renders a new frame at the correct size, but the stretched frame was already displayed → visible wobble

**Fix — two parts required:**
1. **`contentsGravity = kCAGravityTopLeft`** — prevents the compositor from stretching old content; pins it to top-left corner instead, so stale frames just get clipped rather than scaled.
2. **`contentsScale = backingScaleFactor`** — ensures drawable pixels map 1:1 to screen pixels on Retina displays. Without this, topLeft gravity causes coordinate mismatch (content appears at wrong scale, clicks land in wrong positions).

**Bonus:** Read actual texture dimensions from the drawable (`msg_send![texture, width/height]`) instead of using cached width/height, because during resize the drawable may not yet match the cached size.

**WARNING — do NOT use `presentsWithTransaction` + `waitUntilScheduled`:**
These were tested and cause frames to not be presented to screen. Events are processed (hit tests pass, state updates correctly) but the visual output freezes. The `contentsGravity + contentsScale` approach alone is sufficient and does not block the event loop.

**General rule:** For flicker-free Metal resize on macOS, use non-scaling content gravity (`topLeft`) with correct `contentsScale`. Avoid synchronous presentation APIs (`presentsWithTransaction`, `waitUntilScheduled`) as they interfere with normal frame delivery in event-driven apps.
