import { useEffect, useState } from 'react'

/**
 * Tailwind's `md` breakpoint, from the other side. `md:` applies at
 * 768px and up, so everything below it is the phone layout.
 */
const MOBILE_QUERY = '(max-width: 767px)'

/**
 * Which layout the screen is, as a value rather than as CSS.
 *
 * `hidden md:block` renders both branches and hides one. That is fine
 * for a badge and wrong for a subtree: React mounts the hidden branch,
 * runs its effects, subscribes its observers and fires its mutations —
 * CSS only decides what is painted.
 *
 * This repo has paid for that twice. The swipeable row used
 * `md:contents` and the shim it left between the virtualizer's
 * `measureElement` ref and the row's content box made adjacent rows
 * overlap; the fix was this hook, local to that file. The app shell
 * still rendered both shells, so on a phone 83% of the DOM was the
 * hidden desktop copy and opening one thread sent two `mark read`
 * writes — one from each tree, measured in a production build.
 *
 * Initialised from `matchMedia` synchronously, not in an effect: a
 * first render at the wrong breakpoint would mount the wrong tree and
 * swap it a frame later, which is both a flash and the double mount
 * this exists to avoid.
 *
 * The cost, stated: crossing 768px now unmounts one tree and mounts the
 * other, so component-local state does not survive it. Rotating a
 * tablet loses scroll position and any composer text newer than the
 * last autosave. Both trees persisting was the only thing the old
 * approach was better at.
 */
export function useIsMobile(): boolean {
  const [isMobile, setIsMobile] = useState(
    () => typeof window !== 'undefined' && window.matchMedia(MOBILE_QUERY).matches
  )
  useEffect(() => {
    const mq = window.matchMedia(MOBILE_QUERY)
    const handler = (e: MediaQueryListEvent) => setIsMobile(e.matches)
    mq.addEventListener('change', handler)
    return () => mq.removeEventListener('change', handler)
  }, [])
  return isMobile
}
