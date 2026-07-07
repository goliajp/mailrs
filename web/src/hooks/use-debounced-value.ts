import { useEffect, useState } from 'react'

/**
 * Return the input value delayed by `delayMs`. When `value` is
 * falsy the delay is skipped and the input is returned immediately —
 * matches the existing behaviour in `chat.tsx` where clearing the
 * search box updates the filters right away.
 */
export function useDebouncedValue<T>(value: T, delayMs: number): T {
  const [debounced, setDebounced] = useState<T>(value)
  useEffect(() => {
    if (!value) {
      setDebounced(value)
      return
    }
    const t = setTimeout(() => setDebounced(value), delayMs)
    return () => clearTimeout(t)
  }, [value, delayMs])
  return debounced
}
