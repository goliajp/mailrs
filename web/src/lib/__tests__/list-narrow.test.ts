import { describe, expect, it } from 'vitest'

import { draftDate, matchesQuery, sortByDate } from '@/lib/list-narrow'

describe('matchesQuery', () => {
  it('an empty query keeps everything', () => {
    expect(matchesQuery('', ['anything'])).toBe(true)
    expect(matchesQuery('   ', ['anything'])).toBe(true)
  })

  it('matches any field, case-insensitively', () => {
    expect(matchesQuery('QUAL', ['re: qualcomm', 'a@b.com'])).toBe(true)
    expect(matchesQuery('zzz', ['re: qualcomm', 'a@b.com'])).toBe(false)
  })
})

describe('sortByDate', () => {
  const rows = [{ d: 100 }, { d: 300 }, { d: 200 }]
  const d = (r: { d: number }) => r.d

  it('newest first by default', () => {
    expect(sortByDate(rows, d, 'newest').map(d)).toEqual([300, 200, 100])
  })

  it('oldest first when asked', () => {
    expect(sortByDate(rows, d, 'oldest').map(d)).toEqual([100, 200, 300])
  })

  it('falls back to newest for the orders these rows cannot have', () => {
    // Neither is arbitrary-order: relevance is a server ranking a locally
    // matched row never gets, and a send/draft cannot be unread.
    expect(sortByDate(rows, d, 'relevance').map(d)).toEqual([300, 200, 100])
    expect(sortByDate(rows, d, 'unread').map(d)).toEqual([300, 200, 100])
  })

  it('does not mutate its input', () => {
    const original = [...rows]
    sortByDate(rows, d, 'oldest')
    expect(rows).toEqual(original)
  })
})

describe('draftDate', () => {
  it('prefers updated_at, falls back to created_at, then zero', () => {
    expect(draftDate({ created_at: 100, updated_at: 200 })).toBe(200)
    expect(draftDate({ created_at: 100, updated_at: null })).toBe(100)
    expect(draftDate({ created_at: null, updated_at: null })).toBe(0)
  })

  it('accepts the ISO string the API also returns', () => {
    expect(draftDate({ created_at: null, updated_at: '2026-08-05T00:00:00Z' })).toBe(1785888000)
  })

  it('is zero rather than NaN for an unparseable stamp', () => {
    expect(draftDate({ created_at: null, updated_at: 'not a date' })).toBe(0)
  })
})
