import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { useVisualViewport } from '../use-visual-viewport'

type Listener = () => void

class MockVisualViewport {
  listeners: Record<string, Listener[]> = { resize: [], scroll: [] }
  addEventListener = vi.fn((evt: string, fn: Listener) => {
    this.listeners[evt]?.push(fn)
  })

  height = 800

  removeEventListener = vi.fn((evt: string, fn: Listener) => {
    this.listeners[evt] = (this.listeners[evt] ?? []).filter((l) => l !== fn)
  })

  emit(evt: string) {
    for (const fn of this.listeners[evt] ?? []) fn()
  }
}

describe('useVisualViewport', () => {
  let vv: MockVisualViewport

  beforeEach(() => {
    vv = new MockVisualViewport()
    Object.defineProperty(window, 'visualViewport', { configurable: true, value: vv })
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 800 })
  })

  afterEach(() => {
    Object.defineProperty(window, 'visualViewport', { configurable: true, value: undefined })
  })

  it('returns initial state with no keyboard', () => {
    const { result } = renderHook(() => useVisualViewport())

    expect(result.current.isKeyboardOpen).toBe(false)
    expect(result.current.keyboardHeight).toBe(0)
    expect(result.current.viewportHeight).toBe(800)
  })

  it('detects keyboard open when viewport shrinks past threshold', () => {
    const { result } = renderHook(() => useVisualViewport())

    act(() => {
      vv.height = 500 // window 800 - viewport 500 = 300px keyboard, > 100 threshold
      vv.emit('resize')
    })

    expect(result.current.isKeyboardOpen).toBe(true)
    expect(result.current.keyboardHeight).toBe(300)
    expect(result.current.viewportHeight).toBe(500)
  })

  it('does not flag keyboard open below threshold', () => {
    const { result } = renderHook(() => useVisualViewport())

    act(() => {
      vv.height = 750 // 50px gap, below 100 threshold
      vv.emit('resize')
    })

    expect(result.current.isKeyboardOpen).toBe(false)
    expect(result.current.keyboardHeight).toBe(50)
  })

  it('clamps keyboardHeight to 0 when viewport exceeds window', () => {
    const { result } = renderHook(() => useVisualViewport())

    act(() => {
      vv.height = 900 // larger than window — keyboardHeight should clamp to 0
      vv.emit('resize')
    })

    expect(result.current.keyboardHeight).toBe(0)
    expect(result.current.isKeyboardOpen).toBe(false)
  })

  it('responds to scroll events too', () => {
    const { result } = renderHook(() => useVisualViewport())

    act(() => {
      vv.height = 400
      vv.emit('scroll')
    })

    expect(result.current.viewportHeight).toBe(400)
  })

  it('cleans up listeners on unmount', () => {
    const { unmount } = renderHook(() => useVisualViewport())

    unmount()

    expect(vv.removeEventListener).toHaveBeenCalledWith('resize', expect.any(Function))
    expect(vv.removeEventListener).toHaveBeenCalledWith('scroll', expect.any(Function))
  })

  it('returns initial state when visualViewport is unavailable', () => {
    Object.defineProperty(window, 'visualViewport', { configurable: true, value: undefined })

    const { result } = renderHook(() => useVisualViewport())

    expect(result.current.isKeyboardOpen).toBe(false)
    expect(result.current.keyboardHeight).toBe(0)
    expect(result.current.viewportHeight).toBe(800)
  })
})
