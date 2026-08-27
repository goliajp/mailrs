import { describe, expect, it } from 'vitest'

import { chordList } from '@/hooks/use-keyboard-nav'
import { isMailListId } from '@/lib/mail-lists'
import { SHORTCUT_GROUPS } from '@/lib/shortcuts'

/**
 * `g` then a letter goes to a list.
 *
 * It used to be a `default:` arm of the same `switch` that handles
 * single keys, and `switch` matches `case 's'` first — so `g s` starred
 * the open thread instead of going to Sent, and left the chord armed
 * because that arm never cleared it.
 */
describe('the g chord', () => {
  it('names a real list for every key it answers', () => {
    for (const key of ['a', 'd', 'i', 's']) {
      const list = chordList(key)
      expect(list, `g ${key} goes nowhere`).not.toBeNull()
      expect(isMailListId(list)).toBe(true)
    }
  })

  it('answers nothing for a key that is not part of one', () => {
    for (const key of ['x', 'Enter', 'g', '1']) {
      expect(chordList(key)).toBeNull()
    }
  })

  /**
   * The help panel and the handler read the same table.
   *
   * The panel advertised `g a` for a "Go to Action" that was never
   * implemented. A shortcut sheet that lies is worse than none — it
   * burns the one moment of curiosity you get.
   */
  it('advertises only chords that exist', () => {
    const advertised = SHORTCUT_GROUPS.flatMap((g) => g.shortcuts)
      .filter((s) => s.keys.length === 2 && s.keys[0] === 'g')
      .map((s) => s.keys[1])
    expect(advertised.length).toBeGreaterThan(0)
    for (const key of advertised) {
      expect(chordList(key), `the sheet offers "g ${key}" and nothing handles it`).not.toBeNull()
    }
  })
})
