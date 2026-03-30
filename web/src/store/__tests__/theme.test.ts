import { createStore } from 'jotai/vanilla'
import { describe, expect, it, vi } from 'vitest'

// gds modules call matchMedia at top-level
vi.hoisted(() => {
  Object.defineProperty(window, 'matchMedia', {
    value: vi.fn().mockReturnValue({
      addEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
      matches: false,
      onchange: null,
      removeEventListener: vi.fn(),
    }),
    writable: true,
  })
})

import { themeAtom } from '../theme'

// theme persistence and DOM class management are now handled by gds useThemeEffect()
// this test only verifies the jotai atom state management

describe('themeAtom', () => {
  it('can be set to "light"', () => {
    const store = createStore()
    store.set(themeAtom, 'light')
    expect(store.get(themeAtom)).toBe('light')
  })

  it('can be set to "dark"', () => {
    const store = createStore()
    store.set(themeAtom, 'dark')
    expect(store.get(themeAtom)).toBe('dark')
  })

  it('can be set to "system"', () => {
    const store = createStore()
    store.set(themeAtom, 'system')
    expect(store.get(themeAtom)).toBe('system')
  })

  it('defaults to "system"', () => {
    const store = createStore()
    expect(store.get(themeAtom)).toBe('system')
  })

  it('each store instance tracks independently', () => {
    const storeA = createStore()
    const storeB = createStore()
    storeA.set(themeAtom, 'dark')
    storeB.set(themeAtom, 'light')
    expect(storeA.get(themeAtom)).toBe('dark')
    expect(storeB.get(themeAtom)).toBe('light')
  })
})
