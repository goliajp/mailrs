import type { WireSentMessage } from '@/wire/schemas/mail'

import { describe, expect, it } from 'vitest'

import { dedupeSentByMessageId } from '@/lib/dedupe-sent'

// Only tests the pure dedupe step, not applyOptimisticSent's RQ side
// effects. Keeps the test file free of the mutation module's transitive
// import chain (wire schemas + RQ + auth store).

function placeholder(over: Partial<WireSentMessage> = {}): WireSentMessage {
  return {
    internal_date: 1784861500,
    message_id: 'ph@golia.jp',
    subject: 'Hi',
    thread_id: 't-9',
    to: 'a@b.com',
    uid: 0,
    ...over,
  }
}

function serverRow(over: Partial<WireSentMessage> = {}): WireSentMessage {
  return {
    internal_date: 1784861475,
    message_id: 'server-real@golia.jp',
    subject: 'Re: server row',
    thread_id: 't-1',
    to: 'x@example.com',
    uid: 30260,
    ...over,
  }
}

describe('dedupeSentByMessageId', () => {
  it('prepends the placeholder to an empty cache', () => {
    const rows = dedupeSentByMessageId(placeholder(), undefined)
    expect(rows.map((m) => m.message_id)).toEqual(['ph@golia.jp'])
    expect(rows[0].uid).toBe(0)
  })

  it('drops a prior row with the same message_id (placeholder wins)', () => {
    const rows = dedupeSentByMessageId(placeholder({ message_id: 'dup@golia.jp' }), [
      serverRow({ message_id: 'dup@golia.jp', uid: 30260 }),
      serverRow({ message_id: 'other@golia.jp', uid: 30259 }),
    ])
    const dup = rows.filter((m) => m.message_id === 'dup@golia.jp')
    expect(dup.length).toBe(1)
    expect(dup[0].uid).toBe(0)
    expect(rows.length).toBe(2)
  })

  it('is idempotent under double invocation', () => {
    const first = dedupeSentByMessageId(placeholder({ message_id: 'race@golia.jp' }), undefined)
    const second = dedupeSentByMessageId(placeholder({ message_id: 'race@golia.jp' }), first)
    const raceRows = second.filter((m) => m.message_id === 'race@golia.jp')
    expect(raceRows.length).toBe(1)
    expect(second.length).toBe(1)
  })

  it('leaves other rows untouched when de-duping', () => {
    const initial = [
      serverRow({ message_id: 'keep-a@golia.jp', uid: 100 }),
      serverRow({ message_id: 'target@golia.jp', uid: 200 }),
      serverRow({ message_id: 'keep-b@golia.jp', uid: 300 }),
    ]
    const rows = dedupeSentByMessageId(placeholder({ message_id: 'target@golia.jp' }), initial)
    expect(rows.map((m) => m.message_id)).toEqual([
      'target@golia.jp',
      'keep-a@golia.jp',
      'keep-b@golia.jp',
    ])
    expect(rows[0].uid).toBe(0)
  })

  it('accepts undefined old (cache miss) without throwing', () => {
    const rows = dedupeSentByMessageId(placeholder(), undefined)
    expect(rows.length).toBe(1)
    expect(rows[0].message_id).toBe('ph@golia.jp')
  })
})
