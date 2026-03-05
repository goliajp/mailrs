import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { formatDate, formatFullDate, formatSize, formatUptime } from '../format'

// helper: convert a Date to a unix timestamp (seconds)
function toTs(d: Date): number {
  return Math.floor(d.getTime() / 1000)
}

// helper: build a Date relative to a fixed "now"
function daysAgo(now: Date, days: number): Date {
  const d = new Date(now)
  d.setDate(d.getDate() - days)
  return d
}

describe('formatDate', () => {
  // fix "now" to a Wednesday so week-boundary tests are deterministic
  // 2024-03-06 12:00:00 local time (Wednesday)
  const FIXED_NOW = new Date(2024, 2, 6, 12, 0, 0) // month is 0-indexed

  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(FIXED_NOW)
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('shows HH:MM for a timestamp earlier today', () => {
    const thisMorning = new Date(2024, 2, 6, 8, 30, 0)
    const result = formatDate(toTs(thisMorning))
    // locale-formatted time — just assert it looks like a time (contains ':')
    expect(result).toMatch(/\d{1,2}:\d{2}/)
    expect(result).not.toBe('Yesterday')
  })

  it('shows HH:MM for a timestamp at midnight today', () => {
    const midnight = new Date(2024, 2, 6, 0, 0, 0)
    const result = formatDate(toTs(midnight))
    expect(result).toMatch(/\d{1,2}:\d{2}/)
  })

  it('returns "Yesterday" for yesterday', () => {
    const yesterday = daysAgo(FIXED_NOW, 1)
    expect(formatDate(toTs(yesterday))).toBe('Yesterday')
  })

  it('returns short weekday name for a date earlier this week (Mon/Tue)', () => {
    // FIXED_NOW is Wednesday; Monday is 2 days ago, Tuesday 1 day ago
    // Tuesday is "Yesterday", so use Monday (2 days ago)
    const monday = daysAgo(FIXED_NOW, 2)
    const result = formatDate(toTs(monday))
    // should be a short weekday string like "Mon"
    expect(result).toMatch(/^(Mon|Tue|Wed|Thu|Fri|Sat|Sun)$/)
  })

  it('returns short weekday name for last Monday (start of same week)', () => {
    // FIXED_NOW is Wed 2024-03-06; week starts Mon 2024-03-04
    const weekStart = new Date(2024, 2, 4, 9, 0, 0) // Monday
    const result = formatDate(toTs(weekStart))
    expect(result).toMatch(/^(Mon|Tue|Wed|Thu|Fri|Sat|Sun)$/)
  })

  it('does not return a weekday for last Sunday (previous week)', () => {
    // Sun 2024-03-03 is the previous week
    const lastSunday = new Date(2024, 2, 3, 10, 0, 0)
    const result = formatDate(toTs(lastSunday))
    // must NOT be a short weekday
    expect(result).not.toMatch(/^(Mon|Tue|Wed|Thu|Fri|Sat|Sun)$/)
    // same year → "Mar 3" style
    expect(result).toMatch(/Mar/)
  })

  it('returns month + day for earlier this year', () => {
    const jan1 = new Date(2024, 0, 1, 10, 0, 0)
    const result = formatDate(toTs(jan1))
    expect(result).toMatch(/Jan/)
    expect(result).toMatch(/1/)
    // should not contain a 4-digit year
    expect(result).not.toMatch(/2024/)
  })

  it('returns abbreviated year for a different year', () => {
    const oldDate = new Date(2022, 5, 15, 10, 0, 0)
    const result = formatDate(toTs(oldDate))
    // should include "22" (2-digit year) and month
    expect(result).toMatch(/22/)
    expect(result).toMatch(/Jun/)
  })
})

describe('formatFullDate', () => {
  it('returns a non-empty string', () => {
    const ts = toTs(new Date(2024, 2, 6, 14, 30, 0))
    const result = formatFullDate(ts)
    expect(typeof result).toBe('string')
    expect(result.length).toBeGreaterThan(0)
  })

  it('includes the year', () => {
    const ts = toTs(new Date(2024, 2, 6, 14, 30, 0))
    const result = formatFullDate(ts)
    expect(result).toMatch(/2024/)
  })

  it('includes hours and minutes', () => {
    const ts = toTs(new Date(2024, 2, 6, 14, 30, 0))
    const result = formatFullDate(ts)
    expect(result).toMatch(/\d{1,2}:\d{2}/)
  })

  it('includes the month name', () => {
    const ts = toTs(new Date(2024, 2, 6, 14, 30, 0))
    const result = formatFullDate(ts)
    expect(result).toMatch(/Mar/)
  })
})

describe('formatSize', () => {
  it('formats bytes under 1 KB', () => {
    expect(formatSize(0)).toBe('0B')
    expect(formatSize(1)).toBe('1B')
    expect(formatSize(1023)).toBe('1023B')
  })

  it('formats exactly 1 KB', () => {
    expect(formatSize(1024)).toBe('1.0KB')
  })

  it('formats kilobytes', () => {
    expect(formatSize(2048)).toBe('2.0KB')
    expect(formatSize(1536)).toBe('1.5KB')
    expect(formatSize(1024 * 1024 - 1)).toMatch(/KB$/)
  })

  it('formats exactly 1 MB', () => {
    expect(formatSize(1024 * 1024)).toBe('1.0MB')
  })

  it('formats megabytes', () => {
    expect(formatSize(2 * 1024 * 1024)).toBe('2.0MB')
    expect(formatSize(1.5 * 1024 * 1024)).toBe('1.5MB')
  })
})

describe('formatUptime', () => {
  it('formats seconds only', () => {
    expect(formatUptime(0)).toBe('0m 0s')
    expect(formatUptime(45)).toBe('0m 45s')
    expect(formatUptime(59)).toBe('0m 59s')
  })

  it('formats minutes and seconds', () => {
    expect(formatUptime(60)).toBe('1m 0s')
    expect(formatUptime(90)).toBe('1m 30s')
    expect(formatUptime(3599)).toBe('59m 59s')
  })

  it('formats hours and minutes (no seconds shown)', () => {
    expect(formatUptime(3600)).toBe('1h 0m')
    expect(formatUptime(3660)).toBe('1h 1m')
    expect(formatUptime(7384)).toBe('2h 3m')
  })

  it('handles large values', () => {
    // 25 hours 30 minutes 15 seconds
    const secs = 25 * 3600 + 30 * 60 + 15
    expect(formatUptime(secs)).toBe('25h 30m')
  })
})
