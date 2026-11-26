import { describe, expect, it } from 'vitest'

import { batchActions, batchExtras, batchSelectionOf } from '@/lib/actions'
import { fitCount } from '@/lib/fit-count'

const SLOT = { gap: 4, size: 32 }

function row(id: string, over: { archived?: boolean; flagged?: boolean; unread?: number } = {}) {
  return {
    archived: over.archived ?? false,
    flagged: over.flagged ?? false,
    thread_id: id,
    unread_count: over.unread ?? 0,
  }
}

describe('what the batch bar offers for a selection', () => {
  const rows = [
    row('read'),
    row('unread', { unread: 2 }),
    row('starred', { flagged: true }),
    row('archived', { archived: true }),
  ]
  const offered = (ids: string[]) => {
    const s = batchSelectionOf(rows, new Set(ids))
    return { extras: batchExtras(s), main: batchActions('other', s) }
  }

  it('offers "read" while anything picked is unread, and "unread" once nothing is', () => {
    expect(offered(['unread']).main).toContain('read')
    expect(offered(['unread']).main).not.toContain('unread')
    expect(offered(['read']).main).toContain('unread')
    expect(offered(['read']).main).not.toContain('read')
  })

  it('keeps "unread" in the menu when read and unread are picked together', () => {
    expect(offered(['read', 'unread'])).toEqual({
      extras: ['unread'],
      main: ['archive', 'trash', 'read', 'star', 'label'],
    })
    expect(offered(['read']).extras).toEqual([])
    expect(offered(['unread']).extras).toEqual([])
  })

  it('takes a star or the archive back only when every picked one has it', () => {
    expect(offered(['starred']).main).toContain('unstar')
    expect(offered(['starred', 'read']).main).toContain('star')
    expect(offered(['archived']).main).toContain('unarchive')
    expect(offered(['archived', 'read']).main).toContain('archive')
  })

  it('reads a picked id with no loaded row as nothing known', () => {
    expect(offered(['gone']).main).toEqual(['archive', 'trash', 'unread', 'star', 'label'])
  })

  it('offers restoring first in the Trash', () => {
    const s = batchSelectionOf(rows, new Set(['unread']))
    expect(batchActions('trash', s)).toEqual(['untrash', 'read'])
  })
})

describe('how many actions go inline', () => {
  it('draws them all when they fit, with no menu slot taken', () => {
    expect(fitCount(176, 5, SLOT, false)).toBe(5)
  })

  it('gives one slot to the menu as soon as one does not fit', () => {
    // 175 px holds four slots: three actions and the menu
    expect(fitCount(175, 5, SLOT, false)).toBe(3)
    expect(fitCount(68, 5, SLOT, false)).toBe(1)
  })

  it('leaves only the menu when not even one action fits beside it', () => {
    expect(fitCount(40, 5, SLOT, false)).toBe(0)
  })

  it('counts the menu slot when the menu is there for another reason', () => {
    expect(fitCount(176, 5, SLOT, true)).toBe(4)
    expect(fitCount(212, 5, SLOT, true)).toBe(5)
  })
})
