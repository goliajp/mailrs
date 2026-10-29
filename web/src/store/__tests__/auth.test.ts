import type { AuthInfo } from '../auth'

import { createStore } from 'jotai/vanilla'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { authAtom, isSignedIn } from '../auth'

const STORAGE_KEY = 'mailrs_auth'

const sampleAuth: AuthInfo = {
  accessible_domains: ['example.com'],
  address: 'user@example.com',
  display_name: 'Test User',
  permissions: [],
}

// localStorage mock that supports all needed methods
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

describe('authAtom', () => {
  let mockStorage: Storage

  beforeEach(() => {
    mockStorage = makeLocalStorageMock()
    vi.stubGlobal('localStorage', mockStorage)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('reads null when localStorage is empty', () => {
    const store = createStore()
    expect(store.get(authAtom)).toBeNull()
  })

  it('reads AuthInfo from localStorage via isSignedIn after set', () => {
    // baseAuthAtom initializes at module load time, so we verify round-trip:
    // set via authAtom → data persists in mockStorage → isSignedIn reads it back
    const store = createStore()
    store.set(authAtom, sampleAuth)
    const stored = JSON.parse(mockStorage.getItem(STORAGE_KEY) ?? 'null')
    expect(stored).toEqual(sampleAuth)
    expect(isSignedIn()).toBe(true)
  })

  it('writes AuthInfo to localStorage when set', () => {
    const store = createStore()
    store.set(authAtom, sampleAuth)
    const stored = JSON.parse(mockStorage.getItem(STORAGE_KEY) ?? 'null')
    expect(stored).toEqual(sampleAuth)
  })

  it('atom value is updated after set', () => {
    const store = createStore()
    store.set(authAtom, sampleAuth)
    expect(store.get(authAtom)).toEqual(sampleAuth)
  })

  it('removes localStorage key when set to null', () => {
    mockStorage.setItem(STORAGE_KEY, JSON.stringify(sampleAuth))
    const store = createStore()
    store.set(authAtom, null)
    expect(mockStorage.getItem(STORAGE_KEY)).toBeNull()
  })

  it('atom value is null after set to null', () => {
    const store = createStore()
    store.set(authAtom, sampleAuth)
    store.set(authAtom, null)
    expect(store.get(authAtom)).toBeNull()
  })
})

describe('isSignedIn', () => {
  let mockStorage: Storage

  beforeEach(() => {
    mockStorage = makeLocalStorageMock()
    vi.stubGlobal('localStorage', mockStorage)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('is false when no profile is stored', () => {
    expect(isSignedIn()).toBe(false)
  })

  it('is true when a profile is stored', () => {
    mockStorage.setItem(STORAGE_KEY, JSON.stringify(sampleAuth))
    expect(isSignedIn()).toBe(true)
  })

  // the session moved to an HttpOnly cookie; a token an older build
  // stored must not stay readable by scripts
  it('drops a token an older build stored, keeping the profile', () => {
    mockStorage.setItem(STORAGE_KEY, JSON.stringify({ ...sampleAuth, token: 'old-token' }))
    expect(isSignedIn()).toBe(true)
    const stored = JSON.parse(mockStorage.getItem(STORAGE_KEY) ?? 'null')
    expect(stored).toEqual(sampleAuth)
    expect(mockStorage.getItem(STORAGE_KEY)).not.toContain('old-token')
  })
})
