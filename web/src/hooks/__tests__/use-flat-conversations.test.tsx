import type { ConversationSummary } from '@/lib/types'
import type { InfiniteData } from '@tanstack/react-query'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderHook } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { conversationKeys } from '@/store/query-keys-v21'

import { useFlatConversations } from '../use-flat-conversations'

function makeConvo(id: string, unread = 0, lastDate = 100): ConversationSummary {
  return {
    archived: false,
    category: 'inbox',
    flagged: false,
    folder: 'INBOX',
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

function wrap(qc: QueryClient) {
  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  )
}

describe('useFlatConversations', () => {
  it("flattens infinite pages into a dedup'd array", () => {
    const qc = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    })
    const seed: InfiniteData<ConversationSummary[]> = {
      pageParams: [undefined, 100],
      pages: [
        [makeConvo('t-1'), makeConvo('t-2')],
        [makeConvo('t-2'), makeConvo('t-3')], // duplicated across pages
      ],
    }
    qc.setQueryData(conversationKeys.infinite({}), seed)
    const { result } = renderHook(() => useFlatConversations({}), { wrapper: wrap(qc) })
    expect(result.current.conversations.map((c) => c.thread_id)).toEqual(['t-1', 't-2', 't-3'])
  })

  it('reflects mutations to the RQ cache without a separate atom sync', () => {
    const qc = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    })
    qc.setQueryData(conversationKeys.infinite({}), {
      pageParams: [undefined],
      pages: [[makeConvo('t-1', 3)]],
    })
    const { rerender, result } = renderHook(() => useFlatConversations({}), {
      wrapper: wrap(qc),
    })
    expect(result.current.conversations[0].unread_count).toBe(3)
    // Simulate a mutation patching the same cache line.
    qc.setQueryData(conversationKeys.infinite({}), {
      pageParams: [undefined],
      pages: [[{ ...makeConvo('t-1', 0) }]],
    })
    rerender()
    expect(result.current.conversations[0].unread_count).toBe(0)
  })
})
