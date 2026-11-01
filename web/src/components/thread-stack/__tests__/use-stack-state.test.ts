import { act, renderHook } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { makeMessage } from '@/components/__tests__/thread-view-fixtures'

import { useStackState } from '../use-stack-state'

const msgs = (unread: number[]) =>
  [1, 2, 3].map((uid) => makeMessage({ flags: unread.includes(uid) ? 0 : 1, uid }))

describe('the open messages of a conversation', () => {
  it('keeps what opened because it was unread after it is marked read', () => {
    const { rerender, result } = renderHook(({ m }) => useStackState('t1', m, null), {
      initialProps: { m: msgs([1]) },
    })
    expect([...result.current.expanded].sort()).toEqual([1, 3])
    // the refetch after marking read brings every message back as read
    rerender({ m: msgs([]) })
    expect([...result.current.expanded].sort()).toEqual([1, 3])
  })

  it('opens a message that arrives while the conversation is open', () => {
    const { rerender, result } = renderHook(({ m }) => useStackState('t1', m, null), {
      initialProps: { m: msgs([]) },
    })
    rerender({ m: [...msgs([]), makeMessage({ flags: 1, uid: 4 })] })
    expect(result.current.expanded.has(4)).toBe(true)
    expect(result.current.expanded.has(1)).toBe(false)
  })

  it('folds and opens on toggle, and starts over in another conversation', () => {
    const { rerender, result } = renderHook(({ id }) => useStackState(id, msgs([]), null), {
      initialProps: { id: 't1' },
    })
    act(() => result.current.toggle(1))
    act(() => result.current.toggle(3))
    expect([...result.current.expanded]).toEqual([1])
    rerender({ id: 't2' })
    expect([...result.current.expanded]).toEqual([3])
  })

  it('opens the message a row names, and shows everything to reach it', () => {
    const { rerender, result } = renderHook(({ f }) => useStackState('t1', msgs([]), f), {
      initialProps: { f: null as null | number },
    })
    expect(result.current.expanded.has(1)).toBe(false)
    rerender({ f: 1 })
    expect(result.current.expanded.has(1)).toBe(true)
    expect(result.current.showingAll).toBe(true)
  })
})
