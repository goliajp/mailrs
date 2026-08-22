import { describe, expect, it } from 'vitest'

import { chipLabel } from '../date-chip'

describe('chipLabel', () => {
  it('gives one shape to dates the writer wrote differently', () => {
    // The row from the 2026-08-21 report: three formats, three days.
    const written = [
      { date: '2026-08-21', datetime: null, text: 'Aug 21 2026' },
      { date: '2026-08-20', datetime: null, text: '2026-08-20' },
      { date: '2026-08-19', datetime: null, text: '2026-08-19' },
    ]
    const labels = written.map(chipLabel)
    // Same shape for all three — the writer's form no longer leaks into
    // the row. The weekday and day differ, of course; what must not
    // differ is the pattern, so that is what is asserted.

    for (const l of labels)
      expect(l.replace(/[A-Za-z\u4e00-\u9fff]+/g, 'X')).toMatch(/^[X, ]*\d{1,2}[X, ]*$/)
    expect(new Set(labels).size).toBe(3)
  })

  it('reads the day in the local calendar, not UTC', () => {
    // `new Date('2026-08-21')` is UTC midnight and renders as the 20th
    // anywhere west of Greenwich. Built from parts, it does not.
    expect(chipLabel({ date: '2026-08-21', datetime: null })).toContain('21')
  })

  it('carries the hour when one was written, and not when it was not', () => {
    const withTime = chipLabel({ date: '2026-08-25', datetime: '2026-08-25T14:00:00' })
    const allDay = chipLabel({ date: '2026-08-25', datetime: null })
    expect(withTime.length).toBeGreaterThan(allDay.length)
    expect(withTime.startsWith(allDay)).toBe(true)
  })

  it('falls back to the day when the time is unreadable', () => {
    expect(chipLabel({ date: '2026-08-25', datetime: '2026-08-25' })).toBe(
      chipLabel({ date: '2026-08-25', datetime: null })
    )
  })
})
