import type { RefObject } from 'react'

import { useEffect } from 'react'

import { persistScroll } from '@/lib/list-scroll'

/** How long the list rests before its scroll position is saved. */
export const SCROLL_SAVE_PAUSE_MS = 150

/**
 * Keep the saved scroll position of the list in `ref` up to date, under
 * the list identity `identity` holds, so a page refresh puts the reader
 * back where they were.
 *
 * Saved once the scrolling pauses, not on every frame of it: the save
 * writes sessionStorage, and on a throttled CPU that write was a visible
 * share of each frame of a fast scroll. A save still pending when the
 * list goes away is made then.
 */
export function useScrollSaver(
  ref: RefObject<HTMLElement | null>,
  identity: RefObject<string>
): void {
  useEffect(() => {
    const el = ref.current
    if (!el) return
    let pending: null | ReturnType<typeof setTimeout> = null
    const save = () => {
      pending = null
      persistScroll(identity.current, el.scrollTop)
    }
    const onScroll = () => {
      if (pending !== null) clearTimeout(pending)
      pending = setTimeout(save, SCROLL_SAVE_PAUSE_MS)
    }
    el.addEventListener('scroll', onScroll, { passive: true })
    return () => {
      el.removeEventListener('scroll', onScroll)
      if (pending !== null) {
        clearTimeout(pending)
        save()
      }
    }
  }, [ref, identity])
}
