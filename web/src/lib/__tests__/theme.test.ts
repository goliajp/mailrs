import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// matchMedia must exist before theme.ts module evaluates (top-level call)
vi.hoisted(() => {
  Object.defineProperty(window, 'matchMedia', {
    value: vi.fn().mockReturnValue({
      addEventListener: vi.fn(),
      matches: false,
      removeEventListener: vi.fn(),
    }),
    writable: true,
  })
})

import { getTheme, setTheme, type ThemeMode } from '../theme'

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

describe('getTheme', () => {
  let mockStorage: Storage

  beforeEach(() => {
    mockStorage = makeLocalStorageMock()
    vi.stubGlobal('localStorage', mockStorage)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('returns "system" when localStorage is empty', () => {
    expect(getTheme()).toBe('system')
  })

  it('returns "light" when stored', () => {
    mockStorage.setItem('mailrs_theme', 'light')
    expect(getTheme()).toBe('light')
  })

  it('returns "dark" when stored', () => {
    mockStorage.setItem('mailrs_theme', 'dark')
    expect(getTheme()).toBe('dark')
  })

  it('returns "system" when stored', () => {
    mockStorage.setItem('mailrs_theme', 'system')
    expect(getTheme()).toBe('system')
  })

  it('returns "system" for invalid stored value', () => {
    mockStorage.setItem('mailrs_theme', 'invalid')
    expect(getTheme()).toBe('system')
  })

  it('returns "system" for empty string', () => {
    mockStorage.setItem('mailrs_theme', '')
    expect(getTheme()).toBe('system')
  })
})

describe('setTheme', () => {
  let mockStorage: Storage

  beforeEach(() => {
    mockStorage = makeLocalStorageMock()
    vi.stubGlobal('localStorage', mockStorage)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('persists "light" to localStorage', () => {
    setTheme('light')
    expect(mockStorage.getItem('mailrs_theme')).toBe('light')
  })

  it('persists "dark" to localStorage', () => {
    setTheme('dark')
    expect(mockStorage.getItem('mailrs_theme')).toBe('dark')
  })

  it('persists "system" to localStorage', () => {
    setTheme('system')
    expect(mockStorage.getItem('mailrs_theme')).toBe('system')
  })

  it('adds "dark" class to documentElement for dark mode', () => {
    setTheme('dark')
    expect(document.documentElement.classList.contains('dark')).toBe(true)
  })

  it('removes "dark" class from documentElement for light mode', () => {
    document.documentElement.classList.add('dark')
    setTheme('light')
    expect(document.documentElement.classList.contains('dark')).toBe(false)
  })

  it('overwrites previous theme value', () => {
    setTheme('dark')
    setTheme('light')
    expect(mockStorage.getItem('mailrs_theme')).toBe('light')
    expect(document.documentElement.classList.contains('dark')).toBe(false)
  })

  it('round-trips with getTheme', () => {
    const modes: ThemeMode[] = ['system', 'light', 'dark']
    for (const mode of modes) {
      setTheme(mode)
      expect(getTheme()).toBe(mode)
    }
  })
})
