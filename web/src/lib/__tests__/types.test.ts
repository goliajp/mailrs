import { describe, expect, it } from 'vitest'

import {
  FLAG_ANSWERED,
  FLAG_DELETED,
  FLAG_DRAFT,
  FLAG_FLAGGED,
  FLAG_SEEN,
} from '../types'

describe('flag constants', () => {
  it('FLAG_SEEN is 1', () => {
    expect(FLAG_SEEN).toBe(1)
  })

  it('FLAG_ANSWERED is 2', () => {
    expect(FLAG_ANSWERED).toBe(2)
  })

  it('FLAG_FLAGGED is 4', () => {
    expect(FLAG_FLAGGED).toBe(4)
  })

  it('FLAG_DELETED is 8', () => {
    expect(FLAG_DELETED).toBe(8)
  })

  it('FLAG_DRAFT is 16', () => {
    expect(FLAG_DRAFT).toBe(16)
  })

  it('all flags are distinct powers of 2', () => {
    const flags = [
      FLAG_SEEN,
      FLAG_ANSWERED,
      FLAG_FLAGGED,
      FLAG_DELETED,
      FLAG_DRAFT,
    ]
    const unique = new Set(flags)
    expect(unique.size).toBe(flags.length)

    for (const f of flags) {
      expect(f).toBeGreaterThan(0)
      // power of 2 check: n & (n - 1) === 0
      expect(f & (f - 1)).toBe(0)
    }
  })

  it('flags can be combined with bitwise OR', () => {
    const combined = FLAG_SEEN | FLAG_FLAGGED
    expect(combined & FLAG_SEEN).toBeTruthy()
    expect(combined & FLAG_FLAGGED).toBeTruthy()
    expect(combined & FLAG_DELETED).toBeFalsy()
  })

  it('flags can be checked independently after combining', () => {
    const all =
      FLAG_SEEN | FLAG_ANSWERED | FLAG_FLAGGED | FLAG_DELETED | FLAG_DRAFT
    expect(all & FLAG_SEEN).toBeTruthy()
    expect(all & FLAG_ANSWERED).toBeTruthy()
    expect(all & FLAG_FLAGGED).toBeTruthy()
    expect(all & FLAG_DELETED).toBeTruthy()
    expect(all & FLAG_DRAFT).toBeTruthy()
  })

  it('bitwise NOT removes a flag', () => {
    const combined = FLAG_SEEN | FLAG_FLAGGED | FLAG_DRAFT
    const removed = combined & ~FLAG_FLAGGED
    expect(removed & FLAG_SEEN).toBeTruthy()
    expect(removed & FLAG_FLAGGED).toBeFalsy()
    expect(removed & FLAG_DRAFT).toBeTruthy()
  })
})
