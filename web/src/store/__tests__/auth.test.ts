import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createStore } from 'jotai/vanilla'

import { authAtom, getToken } from '../auth'
import type { AuthInfo } from '../auth'

const STORAGE_KEY = 'mailrs_auth'

const sampleAuth: AuthInfo = {
  token: 'tok-abc123',
  address: 'user@example.com',
  display_name: 'Test User',
  super_domains: ['example.com'],
}

// localStorage mock that supports all needed methods
function makeLocalStorageMock(): Storage {
  const store: Record<string, string> = {}
  return {
    getItem: (k: string) => store[k] ?? null,
    setItem: (k: string, v: string) => { store[k] = v },
    removeItem: (k: string) => { delete store[k] },
    clear: () => { for (const k in store) delete store[k] },
    key: (n: number) => Object.keys(store)[n] ?? null,
    get length() { return Object.keys(store).length },
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

  it('reads AuthInfo from localStorage via getToken after set', () => {
    // baseAuthAtom initializes at module load time, so we verify round-trip:
    // set via authAtom → data persists in mockStorage → getToken can read it back
    const store = createStore()
    store.set(authAtom, sampleAuth)
    const stored = JSON.parse(mockStorage.getItem(STORAGE_KEY) ?? 'null')
    expect(stored).toEqual(sampleAuth)
    // getToken reads directly from localStorage, not from the atom
    expect(getToken()).toBe(sampleAuth.token)
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

  it('returns null when localStorage contains invalid JSON', () => {
    mockStorage.setItem(STORAGE_KEY, 'not-valid-json{{{')
    const store = createStore()
    expect(store.get(authAtom)).toBeNull()
  })
})

describe('getToken', () => {
  let mockStorage: Storage

  beforeEach(() => {
    mockStorage = makeLocalStorageMock()
    vi.stubGlobal('localStorage', mockStorage)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('returns null when no auth in localStorage', () => {
    expect(getToken()).toBeNull()
  })

  it('returns token string when auth is stored', () => {
    mockStorage.setItem(STORAGE_KEY, JSON.stringify(sampleAuth))
    expect(getToken()).toBe('tok-abc123')
  })

  it('returns null when localStorage contains invalid JSON', () => {
    mockStorage.setItem(STORAGE_KEY, '{{invalid')
    expect(getToken()).toBeNull()
  })

  it('reflects updated token after authAtom is set', () => {
    const store = createStore()
    store.set(authAtom, { ...sampleAuth, token: 'new-token-xyz' })
    expect(getToken()).toBe('new-token-xyz')
  })
})
