// A recall batch's state is not a field: the server stores `pending`,
// `released_at` and `discarded_at`, and a confirmed batch is one that is
// none of the three.

import { describe, expect, it } from 'vitest'

import { recallBatchCount, recallBatchState } from '@/lib/recall-batch'

const base = {
  at: 1_793_700_000,
  confirmed_at: 0,
  conversations: 0,
  discarded_at: 0,
  observation: 'r12',
  pending: false,
  released_at: 0,
  rules: ['brand-lookalike'],
  would_hold: 0,
}

describe('recall batch state', () => {
  it('is pending while the round waits for a decision', () => {
    expect(recallBatchState({ ...base, pending: true, would_hold: 80 })).toBe('pending')
  })

  it('is held once confirmed, or when the round held on its own', () => {
    expect(recallBatchState({ ...base, conversations: 4 })).toBe('held')
  })

  it('is released once put back', () => {
    expect(recallBatchState({ ...base, conversations: 4, released_at: 1_793_800_000 })).toBe(
      'released'
    )
  })

  it('is discarded once a waiting round was dropped', () => {
    expect(recallBatchState({ ...base, discarded_at: 1_793_800_000, would_hold: 80 })).toBe(
      'discarded'
    )
  })
})

describe('recall batch count', () => {
  it('counts what a waiting or discarded round would hold, and what a held batch took back', () => {
    expect(recallBatchCount({ ...base, conversations: 0, pending: true, would_hold: 80 })).toBe(80)
    expect(recallBatchCount({ ...base, conversations: 4, would_hold: 4 })).toBe(4)
    expect(
      recallBatchCount({ ...base, conversations: 0, discarded_at: 1_793_800_000, would_hold: 80 })
    ).toBe(80)
  })
})
