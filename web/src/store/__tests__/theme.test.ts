import { createStore } from 'jotai/vanilla'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// matchMedia and localStorage must exist before theme.ts module evaluates (top-level calls)
vi.hoisted(() => {
  Object.defineProperty(window, 'matchMedia', {
    value: vi.fn().mockReturnValue({
      addEventListener: vi.fn(),
      matches: false,
      removeEventListener: vi.fn(),
    }),
    writable: true,
  })
  // store/theme.ts calls getTheme() at module level which reads localStorage
  const store: Record<string, string> = {}
  Object.defineProperty(window, 'localStorage', {
    value: {
      clear: () => {
        for (const k in store) delete store[k]
      },
      getItem: (k: string) => store[k] ?? null,
      key: (n: number) => Object.keys(store)[n] ?? null,
      get length() {
        return Object.keys(store).length
      },
      removeItem: (k: string) => {
        delete store[k]
      },
      setItem: (k: string, v: string) => {
        store[k] = v
      },
    },
    writable: true,
  })
})

import { themeAtom } from '../theme'

function makeLocalStorageMock(): Storage {
  const store: Record<string, string> = {}
  return {
    clear: () => {
      for (const k in store) delete store[k]
    },
    getItem: (k: string) => store[k] ?? null,
    key: (n: number) => Object.keys(store)[n] ?? null,
    get length() {
      return Object.keys(store).length
    },
    removeItem: (k: string) => {
      delete store[k]
    },
    setItem: (k: string, v: string) => {
      store[k] = v
    },
  } as Storage
}

describe('themeAtom', () => {
  let mockStorage: Storage

  beforeEach(() => {
    mockStorage = makeLocalStorageMock()
    vi.stubGlobal('localStorage', mockStorage)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

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

  it('persists theme to localStorage via setTheme', () => {
    const store = createStore()
    store.set(themeAtom, 'dark')
    expect(mockStorage.getItem('mailrs_theme')).toBe('dark')
  })

  it('applies dark class to document when set to dark', () => {
    const store = createStore()
    store.set(themeAtom, 'dark')
    expect(document.documentElement.classList.contains('dark')).toBe(true)
  })

  it('removes dark class when set to light', () => {
    const store = createStore()
    store.set(themeAtom, 'dark')
    store.set(themeAtom, 'light')
    expect(document.documentElement.classList.contains('dark')).toBe(false)
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
