import { useEffect, useRef, useState } from 'react'

/**
 * Return `value`, changing at most once every `intervalMs`.
 *
 * A change after a quiet `intervalMs` passes straight through; changes
 * closer together than that wait for the rest of the interval, and the
 * last of them always comes through. Falsy values (a cleared search box)
 * pass at once.
 *
 * For the search box this replaces a 200 ms debounce, which held back
 * every search, the first keystroke and a pasted query alike, by the
 * full 200 ms after the typing stopped. On a 170 ms round trip that
 * alone put the first screen of results past its 300 ms budget.
 */
export function useThrottledValue<T>(value: T, intervalMs: number): T {
  const [shown, setShown] = useState<T>(value)
  const lastChange = useRef(Number.NEGATIVE_INFINITY)
  useEffect(() => {
    if (!value) {
      setShown(value)
      return
    }
    const wait = lastChange.current + intervalMs - Date.now()
    if (wait <= 0) {
      lastChange.current = Date.now()
      setShown(value)
      return
    }
    const t = setTimeout(() => {
      lastChange.current = Date.now()
      setShown(value)
    }, wait)
    return () => clearTimeout(t)
  }, [value, intervalMs])
  return shown
}
