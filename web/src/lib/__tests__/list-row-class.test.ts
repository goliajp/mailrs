import { describe, expect, it } from 'vitest'

import { mailRowClass, mailRowStateClass } from '../list-row-class'

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

  /**
   * A row with its own internal structure — the drafts row is a wrapper with
   * a button inside — takes only the state half. It has to say the same
   * thing about a state as the full version, or the two lists disagree
   * about what "selected" looks like, which is the drift being removed.
   */
  it('the state half says the same thing as the whole', () => {
    for (const state of [
      {},
      { selected: true },
      { flagged: true },
      { muted: true },
      { batchMode: true, selected: true },
      { checked: true },
    ]) {
      const whole = mailRowClass(state)
      for (const token of mailRowStateClass(state).split(' ').filter(Boolean)) {
        expect(whole).toContain(token)
      }
    }
  })

  it('the state half carries no layout', () => {
    // Layout belongs to the caller when the row is not itself the button.
    for (const token of ['flex', 'px-4', 'py-2', 'h-16']) {
      expect(mailRowStateClass({ selected: true })).not.toContain(token)
    }
  })

  it('every row is the same height, whatever its state', () => {
    for (const state of [{}, { selected: true }, { flagged: true }, { muted: true }]) {
      // The virtualizer's estimateSize is fixed at this height; a state that
      // changed it would misplace every row below.
      expect(mailRowClass(state)).toContain('h-16')
    }
  })
})
