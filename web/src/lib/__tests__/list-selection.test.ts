import type { SelectableRow } from '@/lib/list-selection'
import type { MailListId } from '@/lib/mail-lists'

import { describe, expect, it } from 'vitest'

import { resolveSelection } from '@/lib/list-selection'
import { MAIL_LISTS } from '@/lib/mail-lists'

/**
 * The three properties that used to be four components' effects racing
 * each other on one global atom.
 */

const LISTS = Object.keys(MAIL_LISTS) as MailListId[]

function rows(...ids: string[]): SelectableRow[] {
  return ids.map((threadId) => ({ threadId, uid: null }))
}

describe('resolveSelection', () => {
  it('a non-empty list always has a current item', () => {
    for (const list of LISTS) {
      expect(resolveSelection(list, null, rows('a', 'b'))).toEqual({ threadId: 'a', uid: null })
    }
  })

  it('an empty list has none', () => {
    for (const list of LISTS) {
      expect(resolveSelection(list, null, [])).toBeNull()
    }
  })

  it('keeps what the user picked, in the list they picked it in', () => {
    const picked = { list: 'inbox' as const, threadId: 'b', uid: null }
    expect(resolveSelection('inbox', picked, rows('a', 'b'))).toEqual(picked)
  })

  /**
   * A thread you replied in is in both Inbox and Send, so a pick carried
   * across a tab switch would look valid on both sides. The list has to
   * be part of the pick for the fallback to fire.
   */
  it('ignores a pick made in another list, even when that row is here too', () => {
    const picked = { list: 'inbox' as const, threadId: 'b', uid: null }
    expect(resolveSelection('send', picked, rows('a', 'b'))).toEqual({ threadId: 'a', uid: null })
  })

  it('falls back to the first row when the picked one is gone', () => {
    const picked = { list: 'inbox' as const, threadId: 'deleted', uid: null }
    expect(resolveSelection('inbox', picked, rows('a'))).toEqual({ threadId: 'a', uid: null })
  })

  /** Send rows are messages, so the pick carries the uid it was made with. */
  it('carries the message a Send row named', () => {
    const picked = { list: 'send' as const, threadId: 'b', uid: 42 }
    const shown: SelectableRow[] = [
      { threadId: 'a', uid: 7 },
      { threadId: 'b', uid: 42 },
    ]
    expect(resolveSelection('send', picked, shown)?.uid).toBe(42)
  })

  it('the Draft list has nothing to select', () => {
    // Its rows open the composer, so `useCurrentListRows` gives it none —
    // auto-selecting one would pop the composer open on arrival.
    expect(resolveSelection('draft', null, [])).toBeNull()
  })
})
