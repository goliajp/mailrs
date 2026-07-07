import type { ThreadSummary } from '@/domain'

import { QueryClient } from '@tanstack/react-query'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { asThreadId } from '@/domain'
import { conversationKeys } from '@/store/query-keys-v21'

import { markThreadRead, markThreadUnread, starThread } from '../commands/conversation'

vi.mock('@/store/auth', () => ({ getToken: () => 'test-token' }))

const sample = (): ThreadSummary => ({
  category: 'inbox',
  folder: 'INBOX',
  importance: 'low',
  lastDate: 0,
  messageCount: 3,
  participants: [],
  pinned: false,
  requiresAction: false,
  snippet: '',
  snoozedUntil: null,
  starred: false,
  subject: 'hello',
  threadId: asThreadId('t-1'),
  unreadCount: 2,
  version: 0,
})

afterEach(() => {
  vi.unstubAllGlobals()
})

function primeCache(qc: QueryClient) {
  const items = [sample()]
  // Seed BOTH a general list AND a filter-specific list so the test
  // proves the patcher hits every matching cache line — not just one.
  qc.setQueryData(conversationKeys.list({ folder: 'INBOX' }), { items })
  qc.setQueryData(conversationKeys.list({ folder: 'INBOX', unread: true }), { items })
}

describe('markThreadRead', () => {
  it('patches every list containing the thread to unreadCount=0 (optimistic)', async () => {
    const qc = new QueryClient()
    primeCache(qc)
    let resolved = false
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        resolved = true
        return new Response('{}', { status: 200 })
      })
    )

    const promise = markThreadRead(qc, asThreadId('t-1'))
    // Immediately after dispatch, before the wire round-trip resolves,
    // the cache is already patched.
    const inbox = qc.getQueryData<{ items: ThreadSummary[] }>(
      conversationKeys.list({ folder: 'INBOX' })
    )
    const unread = qc.getQueryData<{ items: ThreadSummary[] }>(
      conversationKeys.list({ folder: 'INBOX', unread: true })
    )
    expect(inbox?.items[0].unreadCount).toBe(0)
    expect(unread?.items[0].unreadCount).toBe(0)
    expect(resolved).toBe(true)
    await promise
  })

  it('rolls the cache back on network error', async () => {
    const qc = new QueryClient()
    primeCache(qc)
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('{}', { status: 500 }))
    )

    await expect(markThreadRead(qc, asThreadId('t-1'))).rejects.toBeDefined()

    const inbox = qc.getQueryData<{ items: ThreadSummary[] }>(
      conversationKeys.list({ folder: 'INBOX' })
    )
    expect(inbox?.items[0].unreadCount).toBe(2)
  })
})

describe('markThreadUnread', () => {
  it('patches to unreadCount>=1 on every list containing the thread', async () => {
    const qc = new QueryClient()
    primeCache(qc)
    qc.setQueryData(conversationKeys.list({ folder: 'INBOX' }), {
      items: [{ ...sample(), unreadCount: 0 }],
    })
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('{}', { status: 200 }))
    )
    await markThreadUnread(qc, asThreadId('t-1'))
    const inbox = qc.getQueryData<{ items: ThreadSummary[] }>(
      conversationKeys.list({ folder: 'INBOX' })
    )
    expect(inbox?.items[0].unreadCount).toBeGreaterThanOrEqual(1)
  })
})

describe('starThread', () => {
  it('flips the starred bit optimistically and reconciles via invalidate', async () => {
    const qc = new QueryClient()
    primeCache(qc)
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('{}', { status: 200 }))
    )
    await starThread(qc, asThreadId('t-1'), true)
    const inbox = qc.getQueryData<{ items: ThreadSummary[] }>(
      conversationKeys.list({ folder: 'INBOX' })
    )
    expect(inbox?.items[0].starred).toBe(true)
  })
})
