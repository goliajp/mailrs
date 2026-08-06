import { describe, expect, it } from 'vitest'

import { fitHeight, fitScale, MIN_FIT_SCALE } from '@/lib/fit-to-width'

// A phone column: 390 px viewport less the reading pane's padding.
const PHONE = 366
// The reading pane on a laptop.
const DESKTOP = 820

describe('fitScale', () => {
  it('leaves a message that already fits alone', () => {
    expect(fitScale(600, DESKTOP)).toBe(1)
    expect(fitScale(366, PHONE)).toBe(1)
  })

  it('never enlarges a narrow message to fill the pane', () => {
    expect(fitScale(480, DESKTOP)).toBe(1)
  })

  /**
   * The widths a survey of this mailbox actually found, against a phone.
   * Every one of them fits without touching the floor — which is the
   * property the floor's value was chosen for.
   */
  it.each([
    [600, 0.61],
    [640, 0.57],
    [650, 0.56],
    [680, 0.54],
    [700, 0.52],
    [768, 0.48],
  ])('fits a %ipx email into a phone at ~%f', (width, approx) => {
    const s = fitScale(width, PHONE)
    expect(s).toBeCloseTo(approx, 2)
    expect(s).toBeGreaterThan(MIN_FIT_SCALE)
  })

  it('stops shrinking a pathological width rather than smearing it', () => {
    expect(fitScale(3000, PHONE)).toBe(MIN_FIT_SCALE)
  })

  it('answers 1 when it has not been measured yet', () => {
    expect(fitScale(0, PHONE)).toBe(1)
    expect(fitScale(600, 0)).toBe(1)
    expect(fitScale(Number.NaN, PHONE)).toBe(1)
  })
})

describe('fitHeight', () => {
  it('is the scaled height, because a transform does not move layout', () => {
    expect(fitHeight(2000, 0.5)).toBe(1000)
  })

  it('is unchanged at full size', () => {
    expect(fitHeight(2000, 1)).toBe(2000)
  })

  it('is zero before anything has rendered', () => {
    expect(fitHeight(0, 0.5)).toBe(0)
  })
})
