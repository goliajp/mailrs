import { describe, expect, it } from 'vitest'

import {
  isMailListId,
  MAIL_LIST_ROWS,
  MAIL_LISTS,
  type MailListId,
  threadAxesOf,
} from '@/lib/mail-lists'

const ids = Object.keys(MAIL_LISTS) as MailListId[]

/**
 * The registry's job is that the screen treats every list the same way.
 * These are the properties that make that true — each one is a rule some
 * list did not follow before it was written down here.
 */
describe('MAIL_LISTS', () => {
  it('every list is reachable from exactly one chip row', () => {
    const chips = MAIL_LIST_ROWS.flat()
    expect([...chips].sort()).toEqual([...ids].sort())
    expect(new Set(chips).size).toBe(chips.length)
  })

  it('every chip names a real list', () => {
    for (const id of MAIL_LIST_ROWS.flat()) expect(isMailListId(id)).toBe(true)
  })

  it('every list says what it is and what it says when empty', () => {
    for (const id of ids) {
      expect(MAIL_LISTS[id].label.length).toBeGreaterThan(0)
      expect(MAIL_LISTS[id].emptyLabel.length).toBeGreaterThan(0)
    }
  })

  /**
   * Selectability is a declared property rather than a `case` in
   * `useCurrentListRows`, so that the reading pane and the list cannot
   * disagree about whether a row can become the current item.
   */
  it('draft is the one list whose rows cannot be selected', () => {
    const unselectable = ids.filter((id) => !MAIL_LISTS[id].selectable)
    expect(unselectable).toEqual(['draft'])
  })

  it('threadAxesOf answers for the thread lists and only those', () => {
    for (const id of ids) {
      const axes = threadAxesOf(id)
      expect(axes === null).toBe(MAIL_LISTS[id].source.kind !== 'threads')
    }
  })

  /**
   * Archived is cross-folder on purpose: the server drops the folder when
   * `archived` is set, because "archived within Inbox" is not what the
   * tab means. Pinning it here because it is the one axis combination
   * that looks like an omission.
   */
  it('archived asks for archived threads without naming a folder', () => {
    expect(threadAxesOf('archived')).toEqual({ archived: true })
  })
})
