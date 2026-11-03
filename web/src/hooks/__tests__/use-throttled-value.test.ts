import { act, renderHook } from '@testing-library/react'
import { useEffect } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { useThrottledValue } from '@/hooks/use-throttled-value'

beforeEach(() => vi.useFakeTimers())
afterEach(() => vi.useRealTimers())

describe('useThrottledValue', () => {
  // a query typed or pasted after a pause goes to the server at once;
  // the debounce this replaced held it back 200 ms
  it('passes the first change after a pause at once', () => {
    const { rerender, result } = renderHook(({ v }) => useThrottledValue(v, 150), {
      initialProps: { v: '' },
    })
    rerender({ v: 'alphax' })
    expect(result.current).toBe('alphax')
  })

  // the request starts from what was committed; a second commit before it
  // is a whole re-render of every list reader on the critical path
  it('commits the change it passes at once in a single commit', () => {
    const committed: string[] = []
    const { rerender } = renderHook(
      ({ v }) => {
        const shown = useThrottledValue(v, 150)
        useEffect(() => {
          committed.push(shown)
        })
        return shown
      },
      { initialProps: { v: '' } }
    )
    committed.length = 0
    rerender({ v: 'alphax' })
    expect(committed).toEqual(['alphax'])
  })

  it('holds changes closer than the interval, and the last one comes through', () => {
    const { rerender, result } = renderHook(({ v }) => useThrottledValue(v, 150), {
      initialProps: { v: '' },
    })
    rerender({ v: 'a' })
    expect(result.current).toBe('a')
    act(() => vi.advanceTimersByTime(50))
    rerender({ v: 'al' })
    act(() => vi.advanceTimersByTime(50))
    rerender({ v: 'alp' })
    expect(result.current).toBe('a')
    act(() => vi.advanceTimersByTime(49))
    expect(result.current).toBe('a')
    act(() => vi.advanceTimersByTime(1))
    expect(result.current).toBe('alp')
  })

  it('clears at once', () => {
    const { rerender, result } = renderHook(({ v }) => useThrottledValue(v, 150), {
      initialProps: { v: 'alpha' },
    })
    rerender({ v: 'alphax' })
    rerender({ v: '' })
    expect(result.current).toBe('')
  })
})
