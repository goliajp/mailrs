import type { ReactNode } from 'react'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { useConversationsQuery } from '@/hooks/use-mail-queries'

// one second shared by every row: paging by time alone skips the rest of it
function row(n: number) {
  return {
    category: 'inbox',
    cursor: `1700000000:${1000 + n}`,
    last_date: 1_700_000_000,
    message_count: 1,
    participants: ['a@example.test'],
    snippet: '',
    subject: `s${n}`,
    thread_id: `t${n}`,
    unread_count: 0,
  }
}

let asked: string[]
let qc: QueryClient

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={qc}>{children}</QueryClientProvider>
}

beforeEach(() => {
  asked = []
  qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  vi.spyOn(globalThis, 'fetch').mockImplementation(async (input) => {
    const url = String(input instanceof Request ? input.url : input)
    asked.push(url)
    const page = asked.length === 1 ? Array.from({ length: 50 }, (_, i) => row(i)) : [row(50)]
    return new Response(JSON.stringify({ items: page }), {
      headers: { 'content-type': 'application/json' },
      status: 200,
    })
  })
})
afterEach(() => vi.restoreAllMocks())

describe('the conversation list', () => {
  it('asks for the next page after the last row the server named', async () => {
    const { result } = renderHook(() => useConversationsQuery({ folder: 'INBOX' }), { wrapper })
    await waitFor(() => expect(result.current.isSuccess).toBe(true))
    await act(async () => {
      await result.current.fetchNextPage()
    })
    expect(asked).toHaveLength(2)
    expect(asked[1]).toContain(`cursor=${encodeURIComponent('1700000000:1049')}`)
    expect(asked[1]).not.toContain('before=')
  })

  it('does not ask a search for a second page of the same rows', async () => {
    const { result } = renderHook(
      () => useConversationsQuery({ folder: 'INBOX', query: 'invoice' }),
      { wrapper }
    )
    await waitFor(() => expect(result.current.isSuccess).toBe(true))
    expect(result.current.hasNextPage).toBe(false)
  })
})
