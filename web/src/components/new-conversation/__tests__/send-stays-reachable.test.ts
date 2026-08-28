import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

/**
 * Send has to be reachable however long the message is.
 *
 * Reported 2026-08-28 with a screenshot: a message with two blocks of
 * bank details ran past the bottom of the window and the action bar —
 * Send, Cancel, Add block — went with it. The composer's root was
 * `flex flex-1 flex-col` with no `min-h-0`, and a flex child's default
 * `min-height:auto` lets it grow past its parent rather than scroll
 * inside it. The reply box beside it has had the pair since it was
 * written.
 *
 * Asserted on the classes rather than on a rendered height: jsdom lays
 * nothing out, so a height assertion here would pass whatever the
 * classes said. What can be checked is that the three parts are still
 * declared the way the layout depends on — the frame bounded, the
 * middle scrolling, the bar fixed.
 */
function source(name: string): string {
  // From the project root, which is where vitest runs. `import.meta.url`
  // resolved to `/src/...` under this setup and every assertion failed
  // on a missing file rather than on the classes.
  return readFileSync(`src/components/new-conversation/${name}`, 'utf8')
}

describe('the composer keeps its action bar on screen', () => {
  it('bounds the frame instead of letting it grow', () => {
    const src = source('new-conversation.tsx')
    expect(src).toContain('flex h-full min-h-0 flex-1 flex-col')
  })

  it('scrolls the part that grows', () => {
    const src = source('new-conversation.tsx')
    expect(src).toContain('min-h-0 flex-1 overflow-y-auto')
  })

  it('keeps the action bar out of the scroll', () => {
    const src = source('action-bar.tsx')
    expect(src).toMatch(/flex shrink-0 flex-wrap items-center/)
  })
})
