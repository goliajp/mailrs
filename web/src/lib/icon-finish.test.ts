import { describe, expect, it } from 'vitest'

import { finishIcon, LIFTED_DARK } from './icon-finish'

type Px = [number, number, number, number]

const CLEAR: Px = [0, 0, 0, 0]
const WHITE: Px = [255, 255, 255, 255]

function at(rgba: Uint8ClampedArray, side: number, x: number, y: number): number[] {
  const i = (y * side + x) * 4
  return Array.from(rgba.slice(i, i + 4))
}

// a side×side image painted by `paint(x, y, d)`, d being the distance
// from the centre as a fraction of the side
function image(side: number, paint: (x: number, y: number, d: number) => Px): Uint8ClampedArray {
  const out = new Uint8ClampedArray(side * side * 4)
  const c = (side - 1) / 2
  for (let y = 0; y < side; y++) {
    for (let x = 0; x < side; x++) {
      out.set(paint(x, y, Math.hypot(x - c, y - c) / side), (y * side + x) * 4)
    }
  }
  return out
}

const S = 32

describe('finishIcon', () => {
  it('fills the transparent corners of a rounded-square logo with its own colour', () => {
    const blue: Px = [10, 102, 194, 255]
    const px = image(S, (x, y, d) => {
      if (d > 0.6) return CLEAR
      if (Math.abs(x - 16) < 3 && Math.abs(y - 16) < 3) return WHITE
      return blue
    })
    finishIcon(px, S)
    expect(at(px, S, 0, 0)).toEqual(blue)
    expect(at(px, S, 16, 16)).toEqual(WHITE)
  })

  it('finds the background of a rounded square that has padding around it', () => {
    // linkedin's svg leaves about a tenth of the side empty on every edge
    const blue: Px = [7, 120, 182, 255]
    const px = image(S, (x, y) => {
      if (x < 3 || x > 28 || y < 3 || y > 28) return CLEAR
      if (Math.abs(x - 16) < 3 && Math.abs(y - 16) < 3) return WHITE
      return blue
    })
    finishIcon(px, S)
    expect(at(px, S, 0, 0)).toEqual(blue)
    expect(at(px, S, 16, 16)).toEqual(WHITE)
  })

  it('lifts a black disc to grey and keeps its white glyph white', () => {
    const px = image(S, (x, _y, d) => {
      if (d > 0.5) return CLEAR
      if (Math.abs(x - 16) < 3) return WHITE
      return [0, 0, 0, 255]
    })
    finishIcon(px, S)
    expect(at(px, S, 4, 16)).toEqual([...LIFTED_DARK, 255])
    expect(at(px, S, 0, 0)).toEqual([...LIFTED_DARK, 255])
    expect(at(px, S, 16, 16)).toEqual(WHITE)
  })

  it('leaves a coloured disc background alone', () => {
    const red: Px = [200, 30, 30, 255]
    const px = image(S, (_x, _y, d) => {
      if (d > 0.5) return CLEAR
      return red
    })
    finishIcon(px, S)
    expect(at(px, S, 4, 16)).toEqual(red)
  })

  it('treats a ring logo as a glyph and keeps its inside apart from the ring', () => {
    // lg's red ring with a red face drawn on transparency inside it
    const red: Px = [165, 0, 52, 255]
    const px = image(S, (x, y, d) => {
      if (d > 0.5) return CLEAR
      if (d > 0.4) return red
      if (x === 12 && y === 12) return red
      return CLEAR
    })
    finishIcon(px, S)
    expect(at(px, S, 16, 20)).toEqual(WHITE)
    expect(at(px, S, 12, 12)).toEqual(red)
    expect(at(px, S, 0, 0)).toEqual(WHITE)
  })

  it('does not take a clover reaching the edge for a background', () => {
    // four red leaves with gaps between them that reach the centre
    const red: Px = [220, 0, 20, 255]
    const px = image(S, (x, y, d) => {
      if (d > 0.5) return CLEAR
      if (Math.abs(x - y) < 4 || Math.abs(x + y - 31) < 4) return CLEAR
      return red
    })
    finishIcon(px, S)
    expect(at(px, S, 0, 0)).toEqual(WHITE)
  })

  it('puts a dark or mid-tone glyph on white', () => {
    const px = image(S, (x) => {
      if (x === 16) return [106, 92, 255, 255]
      return CLEAR
    })
    finishIcon(px, S)
    expect(at(px, S, 0, 0)).toEqual(WHITE)
  })

  it('puts a light glyph on the lifted grey, not on black', () => {
    const px = image(S, (x) => {
      if (x === 16) return [250, 250, 250, 255]
      return CLEAR
    })
    finishIcon(px, S)
    expect(at(px, S, 0, 0)).toEqual([...LIFTED_DARK, 255])
  })

  it('leaves every pixel opaque', () => {
    const px = image(S, (x) => {
      if (x === 16) return [0, 0, 0, 128]
      return CLEAR
    })
    finishIcon(px, S)
    for (let i = 3; i < px.length; i += 4) expect(px[i]).toBe(255)
  })
})
