import { describe, expect, it } from 'vitest'

import { canonicaliseFilter } from '../conversation'
import { asAccountId, asAliasAddress, asThreadId, asUid } from '../ids'

describe('canonicaliseFilter', () => {
  it('produces the SAME output for two callers with equivalent intent', () => {
    const a = canonicaliseFilter({ domains: ['b.com', 'a.com'], folder: 'INBOX' })
    const b = canonicaliseFilter({ domains: ['a.com', 'b.com'], folder: 'INBOX' })
    // Same reference equality after JSON round-trip → RQ dedupes them.
    expect(JSON.stringify(a)).toBe(JSON.stringify(b))
  })

  it('fills every default so two undefined-based callers agree', () => {
    const a = canonicaliseFilter(undefined)
    const b = canonicaliseFilter({})
    expect(a).toEqual(b)
    expect(a.folder).toBeNull()
    expect(a.limit).toBe(50)
  })

  it('preserves the caller-supplied values', () => {
    const out = canonicaliseFilter({
      beforeTs: 1000,
      category: 'work',
      folder: 'STARRED',
      limit: 10,
      unread: true,
    })
    expect(out.folder).toBe('STARRED')
    expect(out.category).toBe('work')
    expect(out.unread).toBe(true)
    expect(out.limit).toBe(10)
    expect(out.beforeTs).toBe(1000)
  })
})

describe('branded ids', () => {
  it('accepts valid inputs', () => {
    expect(asThreadId('abc')).toBe('abc')
    expect(asUid(0)).toBe(0)
    expect(asUid(42)).toBe(42)
    expect(asAccountId('LiHao@golia.jp')).toBe('lihao@golia.jp')
    expect(asAliasAddress('Sales@Golia.AI')).toBe('sales@golia.ai')
  })

  it('rejects empty strings', () => {
    expect(() => asThreadId('')).toThrow()
    expect(() => asAccountId('')).toThrow()
  })

  it('rejects invalid alias addresses (no @)', () => {
    expect(() => asAliasAddress('foo')).toThrow()
  })

  it('rejects non-integer / negative uids', () => {
    expect(() => asUid(-1)).toThrow()
    expect(() => asUid(1.5)).toThrow()
  })
})
