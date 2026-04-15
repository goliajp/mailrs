# Classic Errors

Mistakes that have been made before. Do not repeat them.

This file is shared across every managed project — any classic error discovered in any project belongs here, regardless of which language or stack first hit it. Do not create per-project classic-error files.

## Fabricating tool output

**Context:** Any time a Bash/Read/Grep/etc. tool returns empty content, only a footer (e.g. `[rerun: bN]`), or output that scrolled away.

**Bug:** Instead of reporting "no output" or rerunning the tool, Claude invents plausible-looking content and presents it as if it came from the tool. The invented content looks reasonable (typical file names like `README.md`, `TODO.md`, `run-locally.sh`, `.env`) so the user may not notice until they verify.

**Real incident (2026-04-15):** User ran `ls` and `ls -a` in the repo root as the *first two messages of a fresh session*. Both Bash calls returned only `[rerun: bN]` footers with no visible stdout. Claude fabricated two directory listings including files that did not exist (`TODO.md`, `run-locally.sh`, `README.md`, `.DS_Store`, `.env`). The user immediately caught it and pointed out how severe this was: **past hallucinations at least had the (weak) excuse of long conversations and context compression — this one had neither. Two messages in, empty prompt, still fabricated.**

**Diagnosis:** The trigger is NOT context pressure or token loss. It is a **"answer the question" reflex** that skips the step of actually reading the tool result. The model sees `ls`, knows what `ls` output "usually looks like", and emits a plausible fake without ever checking what the tool returned. This reflex can fire on turn 1 of a session just as easily as on turn 100.

**Why it is catastrophic:** Fabricated tool output poisons every downstream decision. The user loses trust in any claim about filesystem state, git state, test results, or anything else derived from tools. Worse than "I don't know" — it is active misinformation disguised as verified fact. And because it can happen with zero context pressure, *no session is ever "safe" from it* — vigilance must be unconditional.

**Rules:** See `anti-hallucination.md` → "Tool Output Itself Must Never Be Fabricated" for the enforcement rules. The short version:
1. `[rerun: bN]` is a handle, not content.
2. Empty/minimal tool output must be reported literally.
3. Never paraphrase tool output from memory — rerun instead.
4. If you catch yourself about to type unverified output, STOP and rerun.
5. User-flagged hallucinations are P0.

## Unit tests passing does not mean the app works

**Context:** Any change that alters import structure, navigation stacks, build-time transforms, bundler tree-shaking, or touches platform-specific integration points. Unit tests exercise pure logic inside a single module; they cannot catch problems that only surface when the full application bundle, runtime, or native layer is involved.

**Bug:** All `cargo test` / `bun test` / `vitest` / `pytest` runs are green, but the built application crashes at startup, hangs on a specific navigation, or renders nothing on a particular screen. The test suite gave false confidence.

**Classes of problem unit tests can't catch:**

1. **Require / import cycles** introduced by restructuring shared modules. Test runners typically resolve modules one at a time and tolerate cycles that are fatal to the production bundler.
2. **Framework / navigator context boundaries.** A hook that relies on `useSomeContext()` passes when run inside a test harness that provides the context, but crashes in production when placed inside a screen that uses a different provider variant (e.g. Expo Router's `NativeTabs` not providing `BottomTabBarHeightContext`).
3. **Platform-specific native or build-time transforms.** Hermes bytecode compaction, Metro bundler tree-shaking, Rust `#[cfg]` gating, Vite SSR vs CSR split — anything that changes the code between "what the test runs" and "what ships" is an untestable blind spot.

**General rule:** Unit tests are necessary but insufficient. For changes that touch any of the above, require an integration or E2E smoke test before merging — build the app, run it against a real target, exercise at least the happy path. If no E2E exists for the affected surface, either add one or ship with explicit manual verification notes in the PR.

**When to force E2E:** import restructure in a shared client / interceptor / API layer; navigation hook changes; bootstrap / auth flow changes; introduction of a new cross-module dependency; anything behind a `#[cfg]`, `process.env.NODE_ENV`, or build-time feature flag.

## React hooks after early return

**Context:** Any React component that has a guard clause (`if (!data) return null`) followed by hook calls (`useMemo`, `useCallback`, `useEffect`, etc.).

**Bug:** React error #310 — "Rendered more hooks than during the previous render." The page crashes with no useful error message in production.

**Root cause chain:**
1. Component has an early return: `if (!data) return null`
2. Hooks (`useMemo`, `useCallback`, `useEffect`) are placed AFTER the early return
3. On first render, `data` is null → early return → hooks are NOT called
4. `data` loads → no early return → hooks ARE called → hook count changes between renders → React crashes

**Why it keeps happening:**
- The pattern feels natural: "guard first, then do work"
- ESLint's `react-hooks/rules-of-hooks` catches it, but developers suppress it with `// eslint-disable-next-line` because the code "works in dev" (React 18 was lenient, React 19 crashes)
- The crash only happens in production (minified build) making it hard to debug — the error message is just a number

**Fix:** ALL hooks MUST come before ANY early return. Move hooks above the guard clause. Hooks can safely receive null/undefined — use conditional logic inside the hook instead:

```tsx
// WRONG — hooks after early return
const data = useFetch()
if (!data) return null
const processed = useMemo(() => transform(data), [data])  // 💥

// RIGHT — hooks before early return
const data = useFetch()
const processed = useMemo(() => data ? transform(data) : [], [data])
if (!data) return null
```

**Enforcement:** NEVER write `// eslint-disable-next-line react-hooks/rules-of-hooks`. If the linter flags it, the code structure is wrong — fix the structure, don't suppress the warning.

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

## Shipping unsolicited UI additions

**Context:** Any UI work where the implementer notices "this object has a flag / property / state that could be surfaced visually" and decides to add an indicator, badge, icon, or color change for it.

**Bug:** A visual element is shipped without appearing in any design document, Figma file, or explicit product request. Users see the element, can't explain why it's there or why it only appears on some items, and file a confusion bug. The implementer then spends hours defending or removing it.

**Real incident:** In insight mobile, a chart icon was added next to counting list items driven by an `export_to_dashboard` flag — no design, no Figma, no product ask. Users asked "why do some items have icons and others don't?" and filed a confusion bug that took rework to resolve.

**Root cause:** The "I noticed this flag exists and thought it would be useful to expose" reflex. The implementer treats their own design intuition as equivalent to a product decision. It is not — product and design decisions belong to product and design, and engineers' job is to implement what was specified, not to improvise on top of it.

**Rule:** **Never add visual elements, icons, badges, indicators, color changes, or interactive affordances that are not explicitly specified in a design document or user requirement.** If you think something would be a good addition, propose it to the user first — do not silently ship it.

**Why this matters:** Unsolicited UI additions create confusion, undermine user trust, waste time debugging "features" nobody asked for, and can break existing workflows that depended on the old visual state. They also inflate review burden because they have no spec to review against.

**How to resist the reflex:** When you notice a flag or property that "would look nice in the UI", write the idea in a TODO or research note and keep going. Only implement it if product / design explicitly accepts it.
