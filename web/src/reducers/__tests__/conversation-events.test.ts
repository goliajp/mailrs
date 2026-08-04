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

describe('onNewMessage — the user replying to a thread', () => {
  /**
   * `mirror_send` publishes a NewMessage whose sender is the user's own
   * address. The server's `record_message_arrival` takes `is_own` and
   * deliberately does not advance the thread's date or position; this
   * reducer lifted and re-dated it anyway, so a reply jumped its thread
   * to the top of the Inbox and dropped back on the next refresh.
   */
  it('leaves the row where it is and does not re-date it', () => {
    const qc = new QueryClient()
    primeInfiniteCache(qc)
    const key = conversationKeys.infinite({ folder: 'INBOX' })
    const before = qc.getQueryData<InfiniteData<ConversationSummary[]>>(key)!
    const target = before.pages[0][1]

    onNewMessage(qc, {
      sender: 'LI HAO <me@example.com>',
      snippet: 'my reply',
      subject: 'hi',
      thread_id: 't-2',
      type: 'NewMessage',
      user: 'me@example.com',
    } as NewMessageEvent)

    const after = qc.getQueryData<InfiniteData<ConversationSummary[]>>(key)!
    expect(after.pages[0].map((c) => c.thread_id)).toEqual(['t-1', 't-2', 't-3'])
    expect(after.pages[0][1]).toEqual(target)
  })

  it('still lifts and re-dates a message from anyone else', () => {
    const qc = new QueryClient()
    primeInfiniteCache(qc)
    const key = conversationKeys.infinite({ folder: 'INBOX' })

    onNewMessage(qc, {
      sender: 'Someone <them@example.com>',
      snippet: 'inbound',
      subject: 'hi',
      thread_id: 't-2',
      type: 'NewMessage',
      user: 'me@example.com',
    } as NewMessageEvent)

    const after = qc.getQueryData<InfiniteData<ConversationSummary[]>>(key)!
    expect(after.pages[0].map((c) => c.thread_id)).toEqual(['t-2', 't-1', 't-3'])
    expect(after.pages[0][0].unread_count).toBe(1)
    expect(after.pages[0][0].last_date).toBeGreaterThan(100)
  })

  /**
   * The event stream is at-least-once — the feed consumer re-tails on
   * FEEDRESYNC and on any read error, and the socket reconnects on its
   * own — and the event carries no message identity, so a second
   * delivery is indistinguishable from a second message. Incrementing
   * made that permanent: three crates.io rows showed "3" on 2026-08-04
   * against a server that said 1, and nothing server-side decrements.
   */
  it('applying the same event twice leaves the same row', () => {
    const qc = new QueryClient()
    primeInfiniteCache(qc)
    const key = conversationKeys.infinite({ folder: 'INBOX' })
    const event = {
      sender: 'crates.io <noreply@crates.io>',
      snippet: 'published',
      subject: 'Successfully published',
      thread_id: 't-2',
      type: 'NewMessage',
      user: 'me@example.com',
    } as NewMessageEvent

    onNewMessage(qc, event)
    const once = qc.getQueryData<InfiniteData<ConversationSummary[]>>(key)!.pages[0][0]
    onNewMessage(qc, event)
    onNewMessage(qc, event)
    const thrice = qc.getQueryData<InfiniteData<ConversationSummary[]>>(key)!.pages[0][0]

    expect(thrice.message_count).toBe(once.message_count)
    expect(thrice.unread_count).toBe(once.unread_count)
    expect(thrice.received_count).toBe(once.received_count)
  })
})
