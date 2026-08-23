import { describe, expect, it } from 'vitest'

import { filterLabel, toggledAccounts } from '../account-filter-rules'

const all = ['', 'acc_1', 'acc_2']

describe('narrowing the list to some accounts', () => {
  it('starts with everything and unticking one narrows it', () => {
    expect(toggledAccounts(null, all, 'acc_1')).toEqual(['', 'acc_2'])
  })

  // Back to everything is the parameter absent, not every id in it:
  // the two narrow to the same set, and only one of them is legible.
  it('ticking the last one back returns to no filter at all', () => {
    expect(toggledAccounts(['', 'acc_2'], all, 'acc_1')).toBeNull()
  })

  // A list narrowed to no accounts is a blank screen whose only way
  // back is the control that produced it.
  it('refuses to untick the last one', () => {
    expect(toggledAccounts(['acc_1'], all, 'acc_1')).toEqual(['acc_1'])
  })

  it('says what it is doing', () => {
    expect(filterLabel(null, all)).toBe('All accounts')
    expect(filterLabel(all, all)).toBe('All accounts')
    expect(filterLabel(['acc_1'], all)).toBe('1 of 3 accounts')
  })
})
