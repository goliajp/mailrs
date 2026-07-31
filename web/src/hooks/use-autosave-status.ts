import { useCallback, useRef, useState } from 'react'

/**
 * Tracks whether a periodic autosave is getting through.
 *
 * Both draft autosaves — the full-screen composer and the inline reply —
 * swallowed the save's failure under the comment "transient — the next tick
 * retries". That is true of a dropped connection and false of everything
 * else: a 422
 * from a field the server renamed fails identically on every tick, forever,
 * and the user goes on typing into a box that is not being saved and says
 * nothing. One of those renames was live on production for a week.
 *
 * A retry is still the right response to the first failure. What was missing
 * is the point at which retrying stops being an explanation.
 */

/** Failures in a row before the user is told. Three ticks ≈ 9 seconds. */
export const FAILURES_BEFORE_WARNING = 3

export type AutosaveStatus = {
  /** Consecutive failed attempts. Reset by the next success. */
  readonly consecutiveFailures: number
  /** The last failure's message, for the detail line. */
  readonly lastError: null | string
  /** Call with the save attempt; it settles the counters and rethrows nothing. */
  readonly record: (attempt: () => Promise<void>) => Promise<void>
  /** Whether to tell the user the draft is not being saved. */
  readonly shouldWarn: boolean
}

export function useAutosaveStatus(): AutosaveStatus {
  const [consecutiveFailures, setConsecutiveFailures] = useState(0)
  const [lastError, setLastError] = useState<null | string>(null)

  // The counter is also held in a ref so `record` stays referentially stable —
  // it is called from a bind-once interval.
  const failures = useRef(0)

  const record = useCallback(async (attempt: () => Promise<void>) => {
    try {
      await attempt()
      if (failures.current !== 0) {
        failures.current = 0
        setConsecutiveFailures(0)
        setLastError(null)
      }
    } catch (err) {
      failures.current += 1
      setConsecutiveFailures(failures.current)
      setLastError(err instanceof Error ? err.message : String(err))
    }
  }, [])

  return {
    consecutiveFailures,
    lastError,
    record,
    shouldWarn: consecutiveFailures >= FAILURES_BEFORE_WARNING,
  }
}
