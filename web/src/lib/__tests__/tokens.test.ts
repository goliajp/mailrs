import { describe, expect, it } from 'vitest'

import { colorVars, cssVar, fontSize, fontWeight, radius, spacing, zIndex } from '../tokens'

describe('spacing', () => {
  it('has standard scale values', () => {
    expect(spacing[0]).toBe('0')
    expect(spacing[1]).toBe('0.25rem')
    expect(spacing[4]).toBe('1rem')
    expect(spacing[8]).toBe('2rem')
  })

  it('all values are strings', () => {
    for (const v of Object.values(spacing)) {
      expect(typeof v).toBe('string')
    }
  })
})

describe('radius', () => {
  it('has none through full', () => {
    expect(radius.none).toBe('0')
    expect(radius.full).toBe('9999px')
  })

  it('sm < md < lg', () => {
    expect(parseFloat(radius.sm)).toBeLessThan(parseFloat(radius.md))
    expect(parseFloat(radius.md)).toBeLessThan(parseFloat(radius.lg))
  })
})

describe('fontSize', () => {
  it('xs is smallest, 2xl is largest', () => {
    const xsVal = parseFloat(fontSize.xs)
    const baseVal = parseFloat(fontSize.base)
    const xlVal = parseFloat(fontSize['2xl'])
    expect(xsVal).toBeLessThan(baseVal)
    expect(baseVal).toBeLessThan(xlVal)
  })
})

describe('fontWeight', () => {
  it('has standard weights', () => {
    expect(fontWeight.normal).toBe('400')
    expect(fontWeight.bold).toBe('700')
  })
})

describe('zIndex', () => {
  it('layers stack in correct order', () => {
    expect(zIndex.dropdown).toBeLessThan(zIndex.sticky)
    expect(zIndex.sticky).toBeLessThan(zIndex.overlay)
    expect(zIndex.overlay).toBeLessThan(zIndex.modal)
    expect(zIndex.modal).toBeLessThan(zIndex.popover)
    expect(zIndex.popover).toBeLessThan(zIndex.toast)
  })
})

describe('colorVars', () => {
  it('all start with --color-', () => {
    for (const v of Object.values(colorVars)) {
      expect(v).toMatch(/^--color-/)
    }
  })

  it('has bg, text, border, brand, and status groups', () => {
    expect(colorVars.bgBase).toBeDefined()
    expect(colorVars.textPrimary).toBeDefined()
    expect(colorVars.borderDefault).toBeDefined()
    expect(colorVars.brandPrimary).toBeDefined()
    expect(colorVars.statusSuccess).toBeDefined()
  })
})

describe('cssVar', () => {
  it('wraps name in var()', () => {
    expect(cssVar('--color-bg-base')).toBe('var(--color-bg-base)')
  })
})
