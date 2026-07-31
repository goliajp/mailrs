import { act, renderHook } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { FAILURES_BEFORE_WARNING, useAutosaveStatus } from '../use-autosave-status'

/**
 * The draft autosaves swallowed every failure under "transient — the next
 * tick retries". A 422 is not transient: it fails identically forever while
 * the user keeps typing into a box that is not being saved.
 */
describe('useAutosaveStatus', () => {
  it('counts a rejected save and does not rethrow', async () => {
    const { result } = renderHook(() => useAutosaveStatus())

    await act(async () => {
      // If this rethrew, the bind-once interval would see an unhandled
      // rejection rather than a counted failure.
      await result.current.record(() => Promise.reject(new Error('422 unknown field')))
    })

    expect(result.current.consecutiveFailures).toBe(1)
    expect(result.current.lastError).toBe('422 unknown field')
    expect(result.current.shouldWarn).toBe(false)
  })

  it('warns only once retrying has stopped being an explanation', async () => {
    const { result } = renderHook(() => useAutosaveStatus())

    for (let i = 0; i < FAILURES_BEFORE_WARNING; i += 1) {
      expect(result.current.shouldWarn).toBe(false)
      await act(async () => {
        await result.current.record(() => Promise.reject(new Error('nope')))
      })
    }

    expect(result.current.consecutiveFailures).toBe(FAILURES_BEFORE_WARNING)
    expect(result.current.shouldWarn).toBe(true)
  })

  it('a success clears the warning', async () => {
    const { result } = renderHook(() => useAutosaveStatus())

    for (let i = 0; i < FAILURES_BEFORE_WARNING; i += 1) {
      await act(async () => {
        await result.current.record(() => Promise.reject(new Error('nope')))
      })
    }
    expect(result.current.shouldWarn).toBe(true)

    await act(async () => {
      await result.current.record(() => Promise.resolve())
    })

    expect(result.current.consecutiveFailures).toBe(0)
    expect(result.current.lastError).toBeNull()
    expect(result.current.shouldWarn).toBe(false)
  })

  it('is stable across renders so a bind-once interval keeps working', () => {
    const { rerender, result } = renderHook(() => useAutosaveStatus())
    const first = result.current.record
    rerender()
    expect(result.current.record).toBe(first)
  })
})
