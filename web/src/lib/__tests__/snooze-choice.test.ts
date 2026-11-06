// The four snooze times, as iOS offers them (ios/Mailrs/Wire/SnoozeChoice.swift):
// three hours on, or 8:00 on the reader's own clock tomorrow, on the coming
// Saturday or on the coming Monday. Read in Santa Clara, where the clocks
// go back on Sunday 1 November 2026, so a morning across the change is
// checked in both directions of the offset.

import { describe, expect, it, vi } from 'vitest'

vi.hoisted(() => {
  process.env.TZ = 'America/Los_Angeles'
})

import { SNOOZE_CHOICES, snoozeDate } from '@/lib/snooze-choice'

// local wall-clock in the test's zone
function at(y: number, mo: number, d: number, h: number, mi = 0): Date {
  return new Date(y, mo - 1, d, h, mi)
}

describe('snooze choices', () => {
  it('offers the four iOS offers, in its order', () => {
    expect(SNOOZE_CHOICES.map((c) => c.choice)).toEqual([
      'laterToday',
      'tomorrowMorning',
      'thisWeekend',
      'nextWeek',
    ])
  })

  it('later today is three hours on, even late at night', () => {
    const now = at(2026, 11, 4, 23, 10)
    expect(snoozeDate('laterToday', now).getTime() - now.getTime()).toBe(3 * 3600 * 1000)
  })

  it('tomorrow morning is 8:00 the next day', () => {
    expect(snoozeDate('tomorrowMorning', at(2026, 11, 4, 23, 10))).toEqual(at(2026, 11, 5, 8))
  })

  it('the weekend is the coming Saturday, never today', () => {
    // Wednesday
    expect(snoozeDate('thisWeekend', at(2026, 11, 4, 10))).toEqual(at(2026, 11, 7, 8))
    // on a Saturday it is the next one
    expect(snoozeDate('thisWeekend', at(2026, 11, 7, 6))).toEqual(at(2026, 11, 14, 8))
  })

  it('next week is the coming Monday, never today', () => {
    expect(snoozeDate('nextWeek', at(2026, 11, 4, 10))).toEqual(at(2026, 11, 9, 8))
    // said on a Monday, it is the Monday coming
    expect(snoozeDate('nextWeek', at(2026, 11, 9, 7))).toEqual(at(2026, 11, 16, 8))
    // said on a Sunday, it is tomorrow
    expect(snoozeDate('nextWeek', at(2026, 11, 8, 20))).toEqual(at(2026, 11, 9, 8))
  })

  it('8:00 stays 8:00 on the far side of a clock change, both ways', () => {
    // Saturday 31 October, daylight time; Sunday 1 November is standard time
    const sunday = snoozeDate('tomorrowMorning', at(2026, 10, 31, 12))
    expect(sunday.toISOString()).toBe('2026-11-01T16:00:00.000Z')
    expect(sunday.getHours()).toBe(8)
    // Saturday 13 March 2027, standard time; Sunday 14 March is daylight time
    const spring = snoozeDate('tomorrowMorning', at(2027, 3, 13, 12))
    expect(spring.toISOString()).toBe('2027-03-14T15:00:00.000Z')
    expect(spring.getHours()).toBe(8)
  })

  it('three hours across the change is three hours of real time', () => {
    // 00:30 PDT on 1 November; three hours later the clock reads 02:30 PST
    const now = at(2026, 11, 1, 0, 30)
    const later = snoozeDate('laterToday', now)
    expect(later.getTime() - now.getTime()).toBe(3 * 3600 * 1000)
    expect(later.getHours()).toBe(2)
  })
})
