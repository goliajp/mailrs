import { describe, expect, it } from 'vitest'

import { epochSecondsFromLocalInput } from '../send-mail'

/**
 * The wire wants Unix epoch seconds (`SendRequest.scheduled_at:
 * Option<i64>` in crates/webapi/src/handlers/prefs.rs, and every MCP
 * scheduling tool says so in its description). The composer sent
 * `new Date(v).toISOString()` instead, and neither transport said anything
 * useful: JSON 422'd the request, multipart parsed the string as an i64,
 * failed, dropped it to `None` — and `None` means send now. Every
 * scheduled send went out immediately.
 */
describe('epochSecondsFromLocalInput', () => {
  it('converts a datetime-local value to epoch seconds', () => {
    // No zone in the input, so this is local time by definition — which is
    // what someone picking 09:00 means. Compared against the same
    // construction rather than a fixed number, since the test machine's
    // zone is not knowable here.
    const value = '2026-07-31T09:00'
    const expected = Math.floor(new Date(value).getTime() / 1000)
    expect(epochSecondsFromLocalInput(value)).toBe(expected)
  })

  it('is an integer, not a float', () => {
    const got = epochSecondsFromLocalInput('2026-07-31T09:00:30')
    expect(Number.isInteger(got)).toBe(true)
  })

  it('reads an empty field as not scheduling', () => {
    expect(epochSecondsFromLocalInput('')).toBeNull()
    expect(epochSecondsFromLocalInput('   ')).toBeNull()
  })

  /// `NaN` would survive to `JSON.stringify`, which writes it as `null`,
  /// which the backend reads as "not scheduled" and sends at once — the
  /// original silent failure arriving by a different route.
  it('returns null rather than NaN for input it cannot read', () => {
    for (const bad of ['not a date', '2026-13-45T99:99', 'soon']) {
      const got = epochSecondsFromLocalInput(bad)
      expect(got, `${bad} must not become NaN`).toBeNull()
      expect(JSON.stringify({ scheduled_at: got })).toBe('{"scheduled_at":null}')
    }
  })

  /// The value that used to be sent. Kept as a test so nobody reintroduces
  /// it by "making the type more flexible".
  it('never produces the ISO string the composer used to send', () => {
    const got = epochSecondsFromLocalInput('2026-07-31T09:00')
    expect(typeof got).toBe('number')
    expect(String(got)).not.toContain('T')
  })
})
