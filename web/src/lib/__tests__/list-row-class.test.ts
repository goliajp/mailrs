import { describe, expect, it } from 'vitest'

import { mailRowClass } from '../list-row-class'

/**
 * The Inbox and the Send view are separate components and each had its own
 * row classes. They drifted: the conversation row marks the selected one
 * with an accent left border and a tinted background, and the Send row had
 * neither — so on the Send list nothing showed which message the reading
 * pane was displaying.
 */
describe('mailRowClass', () => {
  it('marks the selected row with the accent border and tint', () => {
    const cls = mailRowClass({ selected: true })
    expect(cls).toContain('border-l-accent')
    expect(cls).toContain('bg-accent/10')
    expect(cls).not.toContain('border-l-transparent')
  })

  it('leaves an unselected row transparent, with hover', () => {
    const cls = mailRowClass({ selected: false })
    expect(cls).toContain('border-l-transparent')
    expect(cls).toContain('hover:bg-bg-secondary')
    expect(cls).not.toContain('border-l-accent')
  })

  /**
   * A send that failed is worth more of the user's attention than which row
   * happens to be open, and the two never need to be distinguished at once.
   */
  it('a flagged row keeps its danger border even when selected', () => {
    const cls = mailRowClass({ flagged: true, selected: true })
    expect(cls).toContain('border-l-danger/60')
    expect(cls).not.toContain('border-l-accent')
    // Still reads as selected through the background.
    expect(cls).toContain('bg-accent/10')
  })

  /** Batch mode carries selection on the checkbox, not the row. */
  it('batch mode suppresses the selected treatment', () => {
    const cls = mailRowClass({ batchMode: true, selected: true })
    expect(cls).toContain('border-l-transparent')
  })

  it('dims a read row only while it is neither selected nor ticked', () => {
    expect(mailRowClass({ muted: true })).toContain('opacity-70')
    expect(mailRowClass({ muted: true, selected: true })).not.toContain('opacity-70')
    expect(mailRowClass({ checked: true, muted: true })).not.toContain('opacity-70')
  })

  it('every row is the same height, whatever its state', () => {
    for (const state of [{}, { selected: true }, { flagged: true }, { muted: true }]) {
      // The virtualizer's estimateSize is fixed at this height; a state that
      // changed it would misplace every row below.
      expect(mailRowClass(state)).toContain('h-16')
    }
  })
})
