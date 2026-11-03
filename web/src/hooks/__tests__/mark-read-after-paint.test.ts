import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mutate = vi.fn()
vi.mock('@/hooks/use-current-list', () => ({
  useConversationRows: () => ({
    rows: [
      { thread_id: 'unread', unread_count: 2 },
      { thread_id: 'read', unread_count: 0 },
    ],
  }),
}))
const mutation = { isPending: false, mutate }
vi.mock('@/hooks/use-mail-mutations', () => ({ useMarkReadMutation: () => mutation }))

const { useMarkReadOnOpen } = await import('@/hooks/use-mark-read-on-open')

beforeEach(() => {
  vi.useFakeTimers({
    toFake: ['setTimeout', 'clearTimeout', 'requestAnimationFrame', 'cancelAnimationFrame'],
  })
  mutate.mockClear()
})
afterEach(() => vi.useRealTimers())

describe('opening an unread conversation marks it read after the paint', () => {
  // the mark re-renders every list holding the row; done in the effect of
  // the click, that re-render came before the conversation was drawn
  it('not in the commit that opened it, but once the next frame is out', () => {
    renderHook(() => useMarkReadOnOpen('unread', true))
    expect(mutate).not.toHaveBeenCalled()
    act(() => vi.advanceTimersToNextFrame())
    expect(mutate).not.toHaveBeenCalled()
    act(() => vi.advanceTimersByTime(1))
    expect(mutate).toHaveBeenCalledWith({ threadId: 'unread' })
  })

  it('not at all when the reader has moved on before the frame', () => {
    const { rerender } = renderHook(({ id }) => useMarkReadOnOpen(id, true), {
      initialProps: { id: 'unread' },
    })
    rerender({ id: 'read' })
    act(() => vi.advanceTimersToNextFrame())
    act(() => vi.advanceTimersByTime(0))
    expect(mutate).not.toHaveBeenCalled()
  })

  it('not for a conversation already read, nor one not on screen', () => {
    renderHook(() => useMarkReadOnOpen('read', true))
    renderHook(() => useMarkReadOnOpen('unread', false))
    act(() => vi.advanceTimersToNextFrame())
    act(() => vi.advanceTimersByTime(0))
    expect(mutate).not.toHaveBeenCalled()
  })
})
