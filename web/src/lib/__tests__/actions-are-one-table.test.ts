// Every surface that offers an action takes it from one table. These check
// the table itself: a pair undoes into each other, a key never means two
// actions on the same conversation, and the shortcuts sheet advertises
// exactly the table's keys.

import { describe, expect, it } from 'vitest'

import {
  actionForKey,
  type ActionId,
  ACTIONS,
  batchActions,
  rowActions,
  type RowState,
} from '@/lib/actions'
import { SHORTCUT_GROUPS } from '@/lib/shortcuts'

const ALL = Object.keys(ACTIONS) as ActionId[]

function* states(): Generator<RowState> {
  for (const archived of [false, true])
    for (const flagged of [false, true])
      for (const pinned of [false, true])
        for (const snoozed of [false, true])
          for (const unread of [false, true])
            for (const view of ['junk', 'np', 'other'] as const)
              yield { archived, flagged, pinned, snoozed, unread, view }
}

describe('the action table', () => {
  it('names each action by its own id', () => {
    for (const id of ALL) expect(ACTIONS[id].id).toBe(id)
  })

  it('asks before only what cannot be undone', () => {
    for (const id of ALL) {
      const s = ACTIONS[id]
      if (s.confirm) expect(s.undo, id).toBeNull()
    }
  })

  it('undoes a toggle into its pair', () => {
    for (const [a, b] of [
      ['archive', 'unarchive'],
      ['star', 'unstar'],
      ['pin', 'unpin'],
      ['read', 'unread'],
      ['snooze', 'unsnooze'],
      ['trash', 'untrash'],
    ] as [ActionId, ActionId][]) {
      expect(ACTIONS[a].undo).toBe(b)
      expect(ACTIONS[b].undo).toBe(a)
    }
  })

  it('offers each conversation one of each pair, and a key means one action', () => {
    for (const s of states()) {
      const offered = rowActions(s)
      expect(new Set(offered).size, JSON.stringify(s)).toBe(offered.length)
      const byKey = new Map<string, ActionId>()
      for (const id of offered) {
        for (const k of ACTIONS[id].keys) {
          expect(byKey.get(k), `${k} in ${JSON.stringify(s)}`).toBeUndefined()
          byKey.set(k, id)
        }
      }
      expect(actionForKey('e', s)).toBe(s.archived ? 'unarchive' : 'archive')
      expect(actionForKey('s', s)).toBe(s.flagged ? 'unstar' : 'star')
      expect(actionForKey('#', s)).toBe('trash')
      expect(actionForKey('l', s)).toBe('label')
    }
  })

  it('moves to the Trash without asking, and deletes for good only from it, asking', () => {
    expect(ACTIONS.trash.confirm).toBe(false)
    expect(ACTIONS['delete-forever'].confirm).toBe(true)
    for (const s of states()) {
      expect(rowActions(s)).not.toContain('delete-forever')
      const inTrash = rowActions({ ...s, view: 'trash' })
      expect(inTrash).toContain('untrash')
      expect(inTrash).not.toContain('trash')
      expect(actionForKey('#', { ...s, view: 'trash' })).toBe('delete-forever')
    }
    expect(batchActions('trash')).toContain('untrash')
    expect(batchActions('trash')).not.toContain('trash')
  })

  it('lets the batch bar offer only what works in a batch', () => {
    for (const view of ['junk', 'np', 'other', 'trash'] as const)
      for (const id of batchActions(view)) expect(ACTIONS[id].batch, id).toBe(true)
  })

  it('advertises in the shortcuts sheet every key the table has', () => {
    const sheet = new Set(SHORTCUT_GROUPS.flatMap((g) => g.shortcuts.flatMap((s) => s.keys)))
    for (const id of ALL)
      for (const k of ACTIONS[id].keys) expect(sheet.has(k), `${id}: ${k}`).toBe(true)
  })
})
