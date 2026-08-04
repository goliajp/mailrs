import type { ThreadMessage } from '@/lib/types'

import { describe, expect, it } from 'vitest'

import { defaultReadingTarget } from '@/lib/thread-reading'

function msg(sender: string): ThreadMessage {
  return { sender, uid: 0 } as unknown as ThreadMessage
}

describe('defaultReadingTarget', () => {
  /**
   * The property that matters. A thread that has messages always has one
   * to show, so the content pane's empty state cannot be reached while a
   * thread with content is open — which is the state a user reached by
   * clicking from an empty list back to the Inbox on 2026-08-04: header
   * and timeline showing the thread, pane saying "Select a message to
   * preview".
   */
  it('is never null for a thread that has messages', () => {
    for (const senders of [
      ['a@x.com'],
      ['me@x.com'],
      ['a@x.com', 'me@x.com'],
      ['me@x.com', 'me@x.com', 'me@x.com'],
    ]) {
      expect(defaultReadingTarget(senders.map(msg), 'me@x.com')).not.toBeNull()
    }
  })

  it('is null only for a thread with no messages', () => {
    expect(defaultReadingTarget([], 'me@x.com')).toBeNull()
  })

  it('picks the last message the user did not send', () => {
    const m = [msg('a@x.com'), msg('me@x.com'), msg('b@x.com'), msg('me@x.com')]
    expect(defaultReadingTarget(m, 'me@x.com')).toBe(2)
  })

  it('falls back to the tail when every message is the users own', () => {
    const m = [msg('me@x.com'), msg('me@x.com')]
    expect(defaultReadingTarget(m, 'me@x.com')).toBe(1)
  })
})
