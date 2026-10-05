import { describe, expect, it } from 'vitest'

import { iconPlate } from './icon-plate'

function pixels(...px: [number, number, number, number][]): Uint8ClampedArray {
  return new Uint8ClampedArray(px.flat())
}

describe('iconPlate', () => {
  it('puts a dark logo on white', () => {
    expect(iconPlate(pixels([30, 30, 30, 255], [0, 0, 0, 0]))).toBe('white')
  })

  it('puts a white logo on black', () => {
    expect(iconPlate(pixels([255, 255, 255, 255], [0, 0, 0, 0]))).toBe('black')
  })

  it('ignores fully transparent pixels whatever their colour', () => {
    expect(iconPlate(pixels([240, 240, 240, 255], [0, 0, 0, 0], [0, 0, 0, 0]))).toBe('black')
  })

  it('weights pixels by opacity', () => {
    // a faint black haze around a solid white glyph stays a white glyph
    expect(iconPlate(pixels([255, 255, 255, 255], [0, 0, 0, 20], [0, 0, 0, 20]))).toBe('black')
  })

  it('puts a saturated blue on white and a yellow on black', () => {
    expect(iconPlate(pixels([0, 0, 255, 255]))).toBe('white')
    expect(iconPlate(pixels([255, 220, 0, 255]))).toBe('black')
  })

  it('falls back to white for an image with no visible pixel', () => {
    expect(iconPlate(pixels([0, 0, 0, 0]))).toBe('white')
  })
})
