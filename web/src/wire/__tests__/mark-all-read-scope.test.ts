import { beforeEach, describe, expect, it, vi } from 'vitest'

import { fixture } from './request-contract-harness'

/**
 * "Mark all as read" marks the list on screen, not the mailbox.
 *
 * The route takes the query string the conversation list takes; pressed
 * inside Notifications it must not silence the inbox, and pressed inside
 * Pinned, a label or the Trash it must not mark the folder around them.
 * It used to send four axes of the list, so those lists marked the whole
 * mailbox. `request_contract_mark_all_read.rs` reads the same fixture.
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

  function params(): Record<string, string> {
    const url = new URL(captured, 'http://mail.test')
    expect(url.pathname).toMatch(/\/conversations\/mark-all-read$/)
    return Object.fromEntries(url.searchParams)
  }

  it('carries every axis of the list, as the shared fixture names them', async () => {
    const { wireMarkAllRead } = await import('../endpoints/mutations')
    await wireMarkAllRead({
      accounts: ['', 'ext_a'],
      archived: true,
      assignedToMe: true,
      category: 'notification',
      folder: 'Inbox',
      followupDueBy: 1700000650,
      includeQuarantined: true,
      label: '3',
      pinned: true,
      section: 'awaiting',
      snoozed: true,
      starred: true,
      trashed: true,
      unread: true,
    })
    expect(params()).toEqual(fixture('conversation-mark-all-read'))
  })

  it('sends what the list itself sends', async () => {
    const { listPath } = await import('@/hooks/use-mail-queries')
    const { wireMarkAllRead } = await import('../endpoints/mutations')
    for (const filters of [
      { folder: 'NonJunk', pinned: true },
      { label: '3' },
      { trashed: true },
      { snoozed: true },
      { section: 'awaiting' },
      { assignedToMe: true },
      { category: 'notification', folder: 'NP', includeQuarantined: true },
    ]) {
      await wireMarkAllRead(filters)
      const list = new URL(listPath(filters), 'http://mail.test').searchParams
      list.delete('limit')
      expect(params()).toEqual(Object.fromEntries(list))
    }
  })

  it('a search typed into the list does not narrow it', async () => {
    const { wireMarkAllRead } = await import('../endpoints/mutations')
    await wireMarkAllRead({ folder: 'Inbox', query: 'invoice' })
    expect(params()).toEqual({ folder: 'Inbox' })
  })

  it('Review is named by its own flag', async () => {
    const { wireMarkAllRead } = await import('../endpoints/mutations')
    await wireMarkAllRead({ includeQuarantined: true, quarantined: true })
    expect(params()).toEqual({ quarantined: 'true' })
  })
})
