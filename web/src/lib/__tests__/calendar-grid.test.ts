import { describe, expect, it } from 'vitest'

import { daysOf, eventsOn, shift, startOfWeek, windowOf } from '@/lib/calendar-grid'

function ev(uid: string, dtstart: null | string, dtend: null | string = null) {
  return { dtend, dtstart, organizer: null, source: null, status: null, summary: uid, uid }
}

describe('calendar grid', () => {
  it('weeks start on Monday', () => {
    // Sunday 6 September 2026
    const monday = startOfWeek(new Date(2026, 8, 6, 15))
    expect(monday).toEqual(new Date(2026, 7, 31))
    expect(startOfWeek(new Date(2026, 7, 31))).toEqual(new Date(2026, 7, 31))
  })

  it('a month shows the six weeks covering it, from its first Monday on or before the 1st', () => {
    const days = daysOf('month', new Date(2026, 8, 17))
    expect(days).toHaveLength(42)
    expect(days[0]).toEqual(new Date(2026, 7, 31))
    expect(windowOf('month', new Date(2026, 8, 17)).end).toEqual(new Date(2026, 9, 12))
  })

  it('a page is a week or a month', () => {
    expect(shift('week', new Date(2026, 8, 3), 1)).toEqual(new Date(2026, 8, 10))
    expect(shift('month', new Date(2026, 0, 31), 1)).toEqual(new Date(2026, 1, 1))
    expect(shift('month', new Date(2026, 0, 31), -1)).toEqual(new Date(2025, 11, 1))
  })

  it('an event is on every day it touches, earliest first, and one without an end lasts an hour', () => {
    const day = new Date(2026, 8, 1)
    const late = new Date(2026, 8, 1, 18).toISOString()
    const early = new Date(2026, 8, 1, 9).toISOString()
    const overnightStart = new Date(2026, 7, 31, 23).toISOString()
    const overnightEnd = new Date(2026, 8, 1, 1).toISOString()
    const yesterday = new Date(2026, 7, 31, 10).toISOString()
    const events = [
      ev('late', late),
      ev('early', early),
      ev('overnight', overnightStart, overnightEnd),
      ev('yesterday', yesterday),
      ev('all-day', null),
    ]
    expect(eventsOn(events, day).map((e) => e.uid)).toEqual(['overnight', 'early', 'late'])
  })
})
