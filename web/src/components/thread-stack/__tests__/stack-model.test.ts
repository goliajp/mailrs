import { describe, expect, it } from 'vitest'

import { makeMessage } from '@/components/__tests__/thread-view-fixtures'

import { defaultExpanded, stackItems } from '../stack-model'

const READ = 1
const UNREAD = 0

/** `n` messages, uids 1..n, read unless listed. */
function conversation(n: number, unread: number[] = []) {
  return Array.from({ length: n }, (_, i) =>
    makeMessage({ flags: unread.includes(i + 1) ? UNREAD : READ, uid: i + 1 })
  )
}

function shape(items: ReturnType<typeof stackItems>): string[] {
  return items.map((it) => {
    if (it.kind === 'hidden') return `+${it.count}`
    if (it.expanded) return `[${it.message.uid}]`
    return String(it.message.uid)
  })
}

describe('which messages open', () => {
  it('opens the newest and every unread one, and nothing else', () => {
    const open = defaultExpanded(conversation(6, [2, 4]), null)
    expect([...open].sort()).toEqual([2, 4, 6])
  })

  it('opens the message the reader was sent to', () => {
    expect(defaultExpanded(conversation(4), 2).has(2)).toBe(true)
  })

  it('ignores a message that is not in the conversation', () => {
    expect([...defaultExpanded(conversation(3), 99)]).toEqual([3])
  })
})

describe('what the stack draws', () => {
  it('folds each run of collapsed older messages into one line', () => {
    const msgs = conversation(8, [4])
    const items = stackItems(msgs, defaultExpanded(msgs, null), false)
    // the first and the newest three stay; the open one breaks the runs;
    // a run of one is just shown
    expect(shape(items)).toEqual(['1', '+2', '[4]', '5', '6', '7', '[8]'])
    // and a second run of two is a second line
    const longer = conversation(9, [4])
    expect(shape(stackItems(longer, defaultExpanded(longer, null), false))).toEqual([
      '1',
      '+2',
      '[4]',
      '+2',
      '7',
      '8',
      '[9]',
    ])
  })

  it('folds nothing in a short conversation', () => {
    const msgs = conversation(5)
    expect(shape(stackItems(msgs, defaultExpanded(msgs, null), false))).toEqual([
      '1',
      '2',
      '3',
      '4',
      '[5]',
    ])
  })

  it('folds nothing once everything was asked for', () => {
    const msgs = conversation(8)
    const items = stackItems(msgs, defaultExpanded(msgs, null), true)
    expect(items.every((it) => it.kind === 'message')).toBe(true)
    expect(items).toHaveLength(8)
  })
})
