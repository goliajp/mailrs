import type { ReactNode } from 'react'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  HOVER_INTENT_MS,
  threadsToPrefetch,
  useHoverPrefetch,
  usePrefetchNeighbours,
} from '@/hooks/use-prefetch-threads'
import { mailKeys } from '@/lib/query-keys'

const rows = ['a', 'b', 'c', 'd', 'e', 'f', 'g'].map((threadId) => ({ threadId, uid: null }))

let qc: QueryClient
let prefetched: string[]

function wrapper({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider client={qc}>
      <Provider store={createStore()}>{children}</Provider>
    </QueryClientProvider>
  )
}

beforeEach(() => {
  qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  prefetched = []
  // the thread fetch itself is the reading pane's; what is under test is
  // which conversations are asked for, and when
  vi.spyOn(qc, 'prefetchQuery').mockImplementation(async (opts) => {
    expect(opts.queryKey[0]).toEqual(mailKeys.thread('x')[0])
    prefetched.push(String(opts.queryKey[opts.queryKey.length - 1]))
  })
})
afterEach(() => {
  vi.restoreAllMocks()
  vi.useRealTimers()
})

describe('threadsToPrefetch', () => {
  it('takes three below, nearest first, then one above', () => {
    expect(threadsToPrefetch(rows, 'c')).toEqual(['d', 'e', 'f', 'b'])
  })
  it('stops at the ends of the list', () => {
    expect(threadsToPrefetch(rows, 'a')).toEqual(['b', 'c', 'd'])
    expect(threadsToPrefetch(rows, 'g')).toEqual(['f'])
  })
  it('fetches nothing for a conversation not in the list', () => {
    expect(threadsToPrefetch(rows, 'zz')).toEqual([])
    expect(threadsToPrefetch(rows, null)).toEqual([])
  })
})

describe('usePrefetchNeighbours', () => {
  it('waits for the open conversation, then fetches its neighbours', () => {
    const { rerender } = renderHook(({ ready }) => usePrefetchNeighbours(rows, 'c', ready), {
      initialProps: { ready: false },
      wrapper,
    })
    expect(prefetched).toEqual([])
    rerender({ ready: true })
    expect(prefetched).toEqual(['d', 'e', 'f', 'b'])
  })

  it('fetches the new neighbours when the selection moves', () => {
    const { rerender } = renderHook(({ at }) => usePrefetchNeighbours(rows, at, true), {
      initialProps: { at: 'a' },
      wrapper,
    })
    rerender({ at: 'b' })
    expect(prefetched).toEqual(['b', 'c', 'd', 'c', 'd', 'e', 'a'])
  })
})

describe('useHoverPrefetch', () => {
  it('fetches a row the pointer rests on', () => {
    vi.useFakeTimers()
    const { result } = renderHook(() => useHoverPrefetch(), { wrapper })
    act(() => result.current.enter('d'))
    act(() => vi.advanceTimersByTime(HOVER_INTENT_MS))
    expect(prefetched).toEqual(['d'])
  })

  it('fetches nothing for rows the pointer only passes over', () => {
    vi.useFakeTimers()
    const { result } = renderHook(() => useHoverPrefetch(), { wrapper })
    act(() => result.current.enter('b'))
    act(() => vi.advanceTimersByTime(HOVER_INTENT_MS - 1))
    act(() => result.current.enter('c'))
    act(() => vi.advanceTimersByTime(HOVER_INTENT_MS - 1))
    act(() => result.current.leave())
    act(() => vi.advanceTimersByTime(HOVER_INTENT_MS))
    expect(prefetched).toEqual([])
  })
})
