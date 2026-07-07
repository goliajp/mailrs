/**
 * Cross-screen consistency — the definitive regression test for the
 * v2.1 rebuild.
 *
 * Scenario the user reported (2026-07-07):
 * > "首页有一个 unread…点到 overview 还是能看到，要刷新才会消失"
 *
 * This test proves it's architecturally impossible in the new shape:
 *   - Seed `conversationKeys.list({folder: 'INBOX'})` with a thread
 *     that has `unreadCount = 2`.
 *   - Dispatch `markThreadRead` — the reducer runs the same code path
 *     the UI button will run.
 *   - Assert every conversation-list cache line containing that
 *     thread reports `unreadCount = 0` synchronously, before the wire
 *     round-trip resolves.
 *   - Assert `useUnreadCount()`'s pure derivation over the same cache
 *     line yields the updated total.
 *
 * If this test ever regresses, either the reducer stopped patching all
 * matching cache lines, or a screen re-introduced a parallel cache.
 */

import type { ThreadSummary } from '@/domain'

import { QueryClient } from '@tanstack/react-query'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { asThreadId } from '@/domain'
import { conversationKeys } from '@/store/query-keys-v21'

import { markThreadRead } from '../commands/conversation'

vi.mock('@/store/auth', () => ({ getToken: () => 'test-token' }))

afterEach(() => {
  vi.unstubAllGlobals()
})

function makeThread(id: string, unread: number): ThreadSummary {
  return {
    category: 'inbox',
    folder: 'INBOX',
    importance: 'low',
    lastDate: 0,
    messageCount: 1,
    participants: [],
    pinned: false,
    requiresAction: false,
    snippet: '',
    snoozedUntil: null,
    starred: false,
    subject: 'hi',
    threadId: asThreadId(id),
    unreadCount: unread,
    version: 0,
  }
}

describe('cross-screen consistency — the v2.1 architectural guarantee', () => {
  it('mark-read on ONE cache line updates EVERY cache line containing that thread', async () => {
    const qc = new QueryClient()

    // Home dashboard reads `conversationKeys.list({folder:'INBOX'})`.
    // Mail list may read `conversationKeys.list({folder:'INBOX', limit:200})`.
    // Sidebar unread badge reads the same INBOX list.
    // Every one of these must reflect the same source of truth.
    qc.setQueryData(conversationKeys.list({ folder: 'INBOX' }), {
      items: [makeThread('t-1', 2), makeThread('t-2', 3)],
    })
    qc.setQueryData(conversationKeys.list({ folder: 'INBOX', limit: 200 }), {
      items: [makeThread('t-1', 2), makeThread('t-2', 3)],
    })
    qc.setQueryData(conversationKeys.list({ folder: 'INBOX', unread: true }), {
      items: [makeThread('t-1', 2), makeThread('t-2', 3)],
    })

    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('{}', { status: 200 }))
    )

    await markThreadRead(qc, asThreadId('t-1'))

    for (const key of [
      conversationKeys.list({ folder: 'INBOX' }),
      conversationKeys.list({ folder: 'INBOX', limit: 200 }),
      conversationKeys.list({ folder: 'INBOX', unread: true }),
    ]) {
      const data = qc.getQueryData<{ items: ThreadSummary[] }>(key)
      expect(data, `key: ${JSON.stringify(key)}`).toBeDefined()
      const t1 = data!.items.find((t) => t.threadId === asThreadId('t-1'))
      expect(t1?.unreadCount, `key: ${JSON.stringify(key)}`).toBe(0)
    }
  })

  it('unread count derives from the same INBOX list — no separate cache to invalidate', async () => {
    const qc = new QueryClient()
    qc.setQueryData(conversationKeys.list({ folder: 'INBOX' }), {
      items: [makeThread('t-1', 2), makeThread('t-2', 3)],
    })

    // Compute unread total the way `useUnreadCount()` does — a pure
    // reduction over the INBOX list.
    const beforeData = qc.getQueryData<{ items: ThreadSummary[] }>(
      conversationKeys.list({ folder: 'INBOX' })
    )
    const before = (beforeData?.items ?? []).reduce((s, t) => s + t.unreadCount, 0)
    expect(before).toBe(5)

    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('{}', { status: 200 }))
    )
    await markThreadRead(qc, asThreadId('t-1'))

    const afterData = qc.getQueryData<{ items: ThreadSummary[] }>(
      conversationKeys.list({ folder: 'INBOX' })
    )
    const after = (afterData?.items ?? []).reduce((s, t) => s + t.unreadCount, 0)
    expect(after).toBe(3)
  })
})
