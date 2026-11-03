import { useEffect, useState } from 'react'

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
 *
 * A change that passes at once is taken in the render that sees it (a
 * state update during render, which React redoes before committing), not
 * in an effect after it: an effect would cost a second commit of every
 * component reading the value before the search request could start.
 */
export function useThrottledValue<T>(value: T, intervalMs: number): T {
  const [state, setState] = useState({ at: Number.NEGATIVE_INFINITY, shown: value })
  let current = state
  if (!Object.is(value, state.shown) && passesNow(value, state.at, intervalMs)) {
    current = { at: stamp(value, state.at), shown: value }
    setState(current)
  }
  const { at, shown } = current
  useEffect(() => {
    if (Object.is(value, shown)) return
    const t = setTimeout(
      () => setState({ at: Date.now(), shown: value }),
      at + intervalMs - Date.now()
    )
    return () => clearTimeout(t)
  }, [value, shown, at, intervalMs])
  return shown
}

function passesNow(value: unknown, at: number, intervalMs: number): boolean {
  if (!value) return true
  return Date.now() >= at + intervalMs
}

/** When the shown value last changed; clearing does not count as a change. */
function stamp(value: unknown, at: number): number {
  if (!value) return at
  return Date.now()
}
