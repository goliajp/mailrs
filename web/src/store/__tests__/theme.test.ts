import { createStore } from 'jotai/vanilla'
import { describe, expect, it, vi } from 'vitest'

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

import { themeModeAtom } from '../theme'

describe('themeModeAtom', () => {
  it('can be set to "light"', () => {
    const store = createStore()
    store.set(themeModeAtom, 'light')
    expect(store.get(themeModeAtom)).toBe('light')
  })

  it('can be set to "dark"', () => {
    const store = createStore()
    store.set(themeModeAtom, 'dark')
    expect(store.get(themeModeAtom)).toBe('dark')
  })

  it('can be set to "system"', () => {
    const store = createStore()
    store.set(themeModeAtom, 'system')
    expect(store.get(themeModeAtom)).toBe('system')
  })

  it('defaults to "system"', () => {
    const store = createStore()
    expect(store.get(themeModeAtom)).toBe('system')
  })

  it('each store instance tracks independently', () => {
    const storeA = createStore()
    const storeB = createStore()
    storeA.set(themeModeAtom, 'dark')
    storeB.set(themeModeAtom, 'light')
    expect(storeA.get(themeModeAtom)).toBe('dark')
    expect(storeB.get(themeModeAtom)).toBe('light')
  })
})
