import { describe, expect, it } from 'vitest'

import { formatDate, formatFullDate, formatSize, formatUptime } from '../format'

function toTs(d: Date): number {
  return Math.floor(d.getTime() / 1000)
}

describe('formatSize edge cases', () => {
  it('handles negative values gracefully', () => {
    // negative bytes do not make sense but should not crash
    const result = formatSize(-1)
    expect(typeof result).toBe('string')
  })

  it('formats very large values in MB', () => {
    const gb = 1024 * 1024 * 1024
    expect(formatSize(gb)).toBe('1024.0MB')
  })

  it('formats fractional KB values', () => {
    expect(formatSize(1234)).toBe('1.2KB')
  })

  it('formats exactly 1023 bytes', () => {
    expect(formatSize(1023)).toBe('1023B')
  })

  it('formats 0 bytes', () => {
    expect(formatSize(0)).toBe('0B')
  })

  it('formats just above 1 MB', () => {
    expect(formatSize(1024 * 1024 + 1)).toMatch(/^1\.0MB$/)
  })
})

describe('formatUptime edge cases', () => {
  it('handles 0 seconds', () => {
    expect(formatUptime(0)).toBe('0m 0s')
  })

  it('handles exactly one minute', () => {
    expect(formatUptime(60)).toBe('1m 0s')
  })

  it('handles exactly one hour', () => {
    expect(formatUptime(3600)).toBe('1h 0m')
  })

  it('handles 24 hours', () => {
    expect(formatUptime(86400)).toBe('24h 0m')
  })

  it('handles fractional-like boundary at 59 seconds', () => {
    expect(formatUptime(59)).toBe('0m 59s')
  })

  it('handles 3599 (just under 1 hour)', () => {
    expect(formatUptime(3599)).toBe('59m 59s')
  })

  it('drops seconds when hours > 0', () => {
    // 1h 2m 30s -> should show 1h 2m (no seconds)
    expect(formatUptime(3750)).toBe('1h 2m')
  })
})

describe('formatDate edge cases', () => {
  it('handles timestamp 0 (epoch)', () => {
    const result = formatDate(0)
    expect(typeof result).toBe('string')
    expect(result.length).toBeGreaterThan(0)
  })

  it('handles very large timestamp', () => {
    // year 2099
    const future = toTs(new Date(2099, 11, 31))
    const result = formatDate(future)
    expect(typeof result).toBe('string')
  })

  it('returns a non-empty string for any valid timestamp', () => {
    const timestamps = [0, 1000000, 1700000000, 2000000000]
    for (const ts of timestamps) {
      const result = formatDate(ts)
      expect(result.length).toBeGreaterThan(0)
    }
  })
})

describe('formatFullDate edge cases', () => {
  it('handles epoch timestamp', () => {
    const result = formatFullDate(0)
    expect(typeof result).toBe('string')
    // epoch is Jan 1, 1970
    expect(result).toMatch(/1970/)
  })

  it('includes all expected components', () => {
    const ts = toTs(new Date(2024, 6, 15, 10, 30, 0))
    const result = formatFullDate(ts)
    // should contain year, month, time
    expect(result).toMatch(/2024/)
    expect(result).toMatch(/Jul/)
    expect(result).toMatch(/\d{1,2}:\d{2}/)
  })
})
