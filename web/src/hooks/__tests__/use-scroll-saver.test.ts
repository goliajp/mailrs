import { renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { SCROLL_SAVE_PAUSE_MS, useScrollSaver } from '@/hooks/use-scroll-saver'
import { readSavedScroll } from '@/lib/list-scroll'

let el: HTMLDivElement
let writes: number

beforeEach(() => {
  vi.useFakeTimers()
  el = document.createElement('div')
  // jsdom lays nothing out, so an element cannot scroll on its own
  let top = 0
  Object.defineProperty(el, 'scrollTop', { get: () => top, set: (v: number) => (top = v) })
  writes = 0
  const store = new Map<string, string>()
  vi.stubGlobal('sessionStorage', {
    getItem: (k: string) => store.get(k) ?? null,
    removeItem: (k: string) => store.delete(k),
    setItem: (k: string, v: string) => {
      writes += 1
      store.set(k, v)
    },
  })
})
afterEach(() => {
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

function scrollTo(top: number) {
  el.scrollTop = top
  el.dispatchEvent(new Event('scroll'))
}

describe('useScrollSaver', () => {
  it('saves once the scrolling pauses, not on every scroll event', () => {
    renderHook(() => useScrollSaver({ current: el }, { current: 'inbox-a' }))
    // a fast scroll: a scroll event every frame for a third of a second
    for (let top = 100; top <= 2000; top += 100) {
      scrollTo(top)
      vi.advanceTimersByTime(16)
    }
    expect(writes).toBe(0)
    vi.advanceTimersByTime(SCROLL_SAVE_PAUSE_MS)
    expect(writes).toBe(1)
    expect(readSavedScroll('inbox-a')).toBe(2000)
  })

  it('saves a pending position when the list goes away', () => {
    const { unmount } = renderHook(() => useScrollSaver({ current: el }, { current: 'inbox-b' }))
    scrollTo(640)
    unmount()
    expect(readSavedScroll('inbox-b')).toBe(640)
    vi.advanceTimersByTime(SCROLL_SAVE_PAUSE_MS)
    expect(writes).toBe(1)
  })
})
