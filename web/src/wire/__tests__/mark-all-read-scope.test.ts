import { beforeEach, describe, expect, it, vi } from 'vitest'

/**
 * "Mark all as read" marks the list on screen, not the mailbox.
 *
 * The route takes the same axes the conversation list takes; pressed
 * inside Notifications it must not silence the inbox. Verified against
 * production on 2026-08-12: notifications 5 → 0 while promotions stayed
 * at 1, which is the only observation that tells the two apart.
 */
describe('wireMarkAllRead scope', () => {
  let captured = ''

  beforeEach(() => {
    vi.resetModules()
    captured = ''
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        captured = url
        return new Response(JSON.stringify({ flipped: 3, success: true }), {
          headers: { 'content-type': 'application/json' },
          status: 200,
        })
      })
    )
  })

  it('carries the list axes as the query the list itself sends', async () => {
    const { wireMarkAllRead } = await import('../endpoints/mutations')
    await wireMarkAllRead({ folder: 'notifications' })
    expect(captured).toContain('/conversations/mark-all-read?folder=notifications')
  })

  it('archived, starred and unread each reach the wire', async () => {
    const { wireMarkAllRead } = await import('../endpoints/mutations')
    await wireMarkAllRead({ archived: true, starred: true, unread: true })
    expect(captured).toContain('archived=true')
    expect(captured).toContain('starred=true')
    expect(captured).toContain('unread=true')
  })

  /**
   * No axes is the whole mailbox — what this did before the route
   * learned to scope, and what the name still says.
   */
  it('no axes sends no query at all', async () => {
    const { wireMarkAllRead } = await import('../endpoints/mutations')
    await wireMarkAllRead()
    expect(captured).toContain('/conversations/mark-all-read')
    expect(captured).not.toContain('?')
  })
})
