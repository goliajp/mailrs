import { describe, expect, it } from 'vitest'

import { formatAbsolute, formatRelative, parseEpochSeconds } from '../format'

describe('parseEpochSeconds', () => {
  it('reads the backend unix-seconds timestamp', () => {
    const date = parseEpochSeconds('1784335208')
    expect(date?.getTime()).toBe(1784335208 * 1000)
  })

  it('returns null for missing or nonsense values instead of 1970', () => {
    expect(parseEpochSeconds('')).toBeNull()
    expect(parseEpochSeconds('0')).toBeNull()
    expect(parseEpochSeconds('not-a-number')).toBeNull()
  })
})

describe('formatAbsolute', () => {
  it('renders zero-padded local YYYY-MM-DD HH:MM', () => {
    expect(formatAbsolute(new Date(2026, 6, 5, 9, 4))).toBe('2026-07-05 09:04')
  })
})

describe('formatRelative', () => {
  const now = new Date(2026, 6, 29, 12, 0, 0)

  it('describes the distance back from now', () => {
    expect(formatRelative(new Date(2026, 6, 29, 11, 59, 30), now)).toBe('just now')
    expect(formatRelative(new Date(2026, 6, 29, 11, 30), now)).toBe('30m ago')
    expect(formatRelative(new Date(2026, 6, 29, 4, 0), now)).toBe('8h ago')
    expect(formatRelative(new Date(2026, 6, 18, 12, 0), now)).toBe('11d ago')
    expect(formatRelative(new Date(2026, 2, 29, 12, 0), now)).toBe('4mo ago')
    expect(formatRelative(new Date(2024, 6, 29, 12, 0), now)).toBe('2y ago')
  })
})
