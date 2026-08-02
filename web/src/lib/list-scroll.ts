// Scroll position per list identity, so switching tabs and coming back
// lands where you left rather than at the top.
//
// Its own module because a file that exports both components and plain
// functions breaks Fast Refresh — `react-refresh/only-export-components`
// says so, and it is right: this is not a component.

const SCROLL_STORAGE_PREFIX = 'chat:list-scroll:'
const savedScrollTops = new Map<string, number>()

export function persistScroll(identity: string, value: number) {
  savedScrollTops.set(identity, value)
  try {
    const key = SCROLL_STORAGE_PREFIX + identity
    if (value > 0) sessionStorage.setItem(key, String(value))
    else sessionStorage.removeItem(key)
  } catch {
    // ignore quota / privacy mode
  }
}

export function readSavedScroll(identity: string): number {
  const cached = savedScrollTops.get(identity)
  if (cached !== undefined) return cached
  try {
    const raw = sessionStorage.getItem(SCROLL_STORAGE_PREFIX + identity)
    const value = raw ? Number(raw) || 0 : 0
    savedScrollTops.set(identity, value)
    return value
  } catch {
    return 0
  }
}
