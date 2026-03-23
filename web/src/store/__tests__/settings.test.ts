import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createStore } from 'jotai/vanilla'

import {
  appendSignature,
  notificationsAtom,
  pageSizeAtom,
  signatureAtom,
  signatureEnabledAtom,
} from '../settings'

function makeLocalStorageMock(): Storage {
  const store: Record<string, string> = {}
  return {
    getItem: (k: string) => store[k] ?? null,
    setItem: (k: string, v: string) => {
      store[k] = v
    },
    removeItem: (k: string) => {
      delete store[k]
    },
    clear: () => {
      for (const k in store) delete store[k]
    },
    key: (n: number) => Object.keys(store)[n] ?? null,
    get length() {
      return Object.keys(store).length
    },
  } as Storage
}

describe('pageSizeAtom', () => {
  let mockStorage: Storage

  beforeEach(() => {
    mockStorage = makeLocalStorageMock()
    vi.stubGlobal('localStorage', mockStorage)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('persists value to localStorage when set', () => {
    const store = createStore()
    store.set(pageSizeAtom, 100)
    expect(store.get(pageSizeAtom)).toBe(100)
    expect(mockStorage.getItem('mailrs_page_size')).toBe('100')
  })

  it('clamps value to minimum 10', () => {
    const store = createStore()
    store.set(pageSizeAtom, 5)
    expect(store.get(pageSizeAtom)).toBe(10)
    expect(mockStorage.getItem('mailrs_page_size')).toBe('10')
  })

  it('clamps value to maximum 200', () => {
    const store = createStore()
    store.set(pageSizeAtom, 500)
    expect(store.get(pageSizeAtom)).toBe(200)
    expect(mockStorage.getItem('mailrs_page_size')).toBe('200')
  })

  it('accepts boundary values 10 and 200', () => {
    const store = createStore()
    store.set(pageSizeAtom, 10)
    expect(store.get(pageSizeAtom)).toBe(10)
    store.set(pageSizeAtom, 200)
    expect(store.get(pageSizeAtom)).toBe(200)
  })

  it('clamps negative values to 10', () => {
    const store = createStore()
    store.set(pageSizeAtom, -1)
    expect(store.get(pageSizeAtom)).toBe(10)
  })
})

describe('notificationsAtom', () => {
  let mockStorage: Storage

  beforeEach(() => {
    mockStorage = makeLocalStorageMock()
    vi.stubGlobal('localStorage', mockStorage)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('persists true to localStorage', () => {
    const store = createStore()
    store.set(notificationsAtom, true)
    expect(store.get(notificationsAtom)).toBe(true)
    expect(mockStorage.getItem('mailrs_notifications')).toBe('true')
  })

  it('persists false to localStorage', () => {
    const store = createStore()
    store.set(notificationsAtom, false)
    expect(store.get(notificationsAtom)).toBe(false)
    expect(mockStorage.getItem('mailrs_notifications')).toBe('false')
  })

  it('toggles between true and false', () => {
    const store = createStore()
    store.set(notificationsAtom, false)
    expect(store.get(notificationsAtom)).toBe(false)
    store.set(notificationsAtom, true)
    expect(store.get(notificationsAtom)).toBe(true)
  })
})

describe('signatureAtom', () => {
  let mockStorage: Storage

  beforeEach(() => {
    mockStorage = makeLocalStorageMock()
    vi.stubGlobal('localStorage', mockStorage)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('persists signature text to localStorage', () => {
    const store = createStore()
    store.set(signatureAtom, 'Best regards,\nAlice')
    expect(store.get(signatureAtom)).toBe('Best regards,\nAlice')
    expect(mockStorage.getItem('mailrs_signature')).toBe('Best regards,\nAlice')
  })

  it('persists empty string', () => {
    const store = createStore()
    store.set(signatureAtom, '')
    expect(store.get(signatureAtom)).toBe('')
    expect(mockStorage.getItem('mailrs_signature')).toBe('')
  })

  it('updates value when set multiple times', () => {
    const store = createStore()
    store.set(signatureAtom, 'first')
    store.set(signatureAtom, 'second')
    expect(store.get(signatureAtom)).toBe('second')
  })
})

describe('signatureEnabledAtom', () => {
  let mockStorage: Storage

  beforeEach(() => {
    mockStorage = makeLocalStorageMock()
    vi.stubGlobal('localStorage', mockStorage)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('persists enabled state to localStorage', () => {
    const store = createStore()
    store.set(signatureEnabledAtom, true)
    expect(store.get(signatureEnabledAtom)).toBe(true)
    expect(mockStorage.getItem('mailrs_signature_enabled')).toBe('true')
  })

  it('persists disabled state to localStorage', () => {
    const store = createStore()
    store.set(signatureEnabledAtom, false)
    expect(store.get(signatureEnabledAtom)).toBe(false)
    expect(mockStorage.getItem('mailrs_signature_enabled')).toBe('false')
  })
})

describe('appendSignature', () => {
  it('returns body unchanged when disabled', () => {
    expect(appendSignature('Hello', 'Best,\nAlice', false)).toBe('Hello')
  })

  it('returns body unchanged when signature is empty', () => {
    expect(appendSignature('Hello', '', true)).toBe('Hello')
  })

  it('returns body unchanged when signature is whitespace only', () => {
    expect(appendSignature('Hello', '   ', true)).toBe('Hello')
  })

  it('appends signature with standard separator when enabled', () => {
    const result = appendSignature('Hello', 'Best regards,\nAlice', true)
    expect(result).toBe('Hello\n\n-- \nBest regards,\nAlice')
  })

  it('uses standard email signature separator "-- "', () => {
    const result = appendSignature('Body', 'Sig', true)
    expect(result).toContain('-- \n')
  })

  it('appends to empty body', () => {
    const result = appendSignature('', 'Sig', true)
    expect(result).toBe('\n\n-- \nSig')
  })

  it('preserves multiline body and signature', () => {
    const body = 'Line 1\nLine 2'
    const sig = 'Name\nTitle\nCompany'
    const result = appendSignature(body, sig, true)
    expect(result).toBe('Line 1\nLine 2\n\n-- \nName\nTitle\nCompany')
  })

  it('returns body when enabled but signature is empty string', () => {
    expect(appendSignature('text', '', true)).toBe('text')
  })
})
