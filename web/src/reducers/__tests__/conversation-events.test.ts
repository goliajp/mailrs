import type { ConversationSummary, NewMessageEvent } from '@/lib/types'
import type { InfiniteData } from '@tanstack/react-query'

import { QueryClient } from '@tanstack/react-query'
import { describe, expect, it } from 'vitest'

import { conversationKeys } from '@/store/query-keys-v21'

import { onConversationRead, onNewMessage } from '../events/conversation'

/**
 * Reducers for server-push events (RFC §3.2). These are pure enough to
 * mount with a bare `QueryClient` — no DOM, no components. If a future
 * refactor breaks any of these, cross-screen updates on WebSocket
 * events regress silently in production.
 */

function makeConvo(id: string, unread: number, lastDate = 100): ConversationSummary {
  return {
    archived: false,
    category: 'inbox',
    flagged: false,
    folder: 'INBOX',
    importance_level: 'low',
    last_date: lastDate,
    last_sender: '',
    message_count: 1,
    participants: [],
    pinned: false,
    received_count: 1,
    snippet: '',
    subject: 'hi',
    thread_id: id,
    unread_count: unread,
  } as unknown as ConversationSummary
}

function primeInfiniteCache(qc: QueryClient) {
  const seed: InfiniteData<ConversationSummary[]> = {
    pageParams: [undefined, 100],
    pages: [[makeConvo('t-1', 0), makeConvo('t-2', 0), makeConvo('t-3', 0)], [makeConvo('t-4', 0)]],
  }
  qc.setQueryData(conversationKeys.infinite({ folder: 'INBOX' }), seed)
}

describe('onNewMessage', () => {
  it('lifts an existing thread to the top of page 0 and bumps its unread + snippet', () => {
    const qc = new QueryClient()
    primeInfiniteCache(qc)
    const event: NewMessageEvent = {
      sender: 'boss@golia.jp',
      snippet: 'let us sync',
      subject: 'Q3 review',
      thread_id: 't-3',
      type: 'NewMessage',
      user: 'a@b.c',
    } as unknown as NewMessageEvent

    onNewMessage(qc, event)

    const cache = qc.getQueryData<InfiniteData<ConversationSummary[]>>(
      conversationKeys.infinite({ folder: 'INBOX' })
    )
    expect(cache).toBeDefined()
    const top = cache!.pages[0][0]
    expect(top.thread_id).toBe('t-3')
    expect(top.unread_count).toBe(1)
    expect(top.snippet).toBe('let us sync')
    expect(top.subject).toBe('Q3 review')
    // The thread was removed from its original position.
    expect(cache!.pages[0].filter((c) => c.thread_id === 't-3')).toHaveLength(1)
  })

  it('invalidates only the specific cache when the thread is not in that filter', () => {
    const qc = new QueryClient()
    primeInfiniteCache(qc)
    let invalidated = false
    // Wrap the client so we can observe invalidation
    const origInvalidate = qc.invalidateQueries.bind(qc)
    qc.invalidateQueries = ((opts: unknown) => {
      const o = opts as { exact?: boolean; queryKey?: unknown[] }
      if (o.exact) invalidated = true
      return origInvalidate(opts as never)
    }) as never

    const event: NewMessageEvent = {
      sender: 'x@y.z',
      snippet: 'new thread',
      subject: 'new',
      thread_id: 't-unknown',
      type: 'NewMessage',
      user: 'a@b.c',
    } as unknown as NewMessageEvent

    onNewMessage(qc, event)
    expect(invalidated).toBe(true)
  })
})

describe('onConversationRead', () => {
  it('sets unread_count to 0 on every cached list line containing the thread', () => {
    const qc = new QueryClient()
    const seed: InfiniteData<ConversationSummary[]> = {
      pageParams: [undefined],
      pages: [[makeConvo('t-1', 3), makeConvo('t-2', 2)]],
    }
    qc.setQueryData(conversationKeys.infinite({ folder: 'INBOX' }), seed)
    qc.setQueryData(conversationKeys.infinite({ folder: 'INBOX', unread: true }), seed)

    onConversationRead(qc, 't-1')

    for (const key of [
      conversationKeys.infinite({ folder: 'INBOX' }),
      conversationKeys.infinite({ folder: 'INBOX', unread: true }),
    ]) {
      const data = qc.getQueryData<InfiniteData<ConversationSummary[]>>(key)
      const t1 = data?.pages.flat().find((c) => c.thread_id === 't-1')
      expect(t1?.unread_count).toBe(0)
    }
  })
})
