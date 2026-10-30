import { createStore } from 'jotai/vanilla'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { appendSignature, notificationsAtom, notificationSoundAtom } from '../settings'

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

describe('notificationSoundAtom', () => {
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
    store.set(notificationSoundAtom, true)
    expect(store.get(notificationSoundAtom)).toBe(true)
    expect(mockStorage.getItem('mailrs_notification_sound')).toBe('true')
  })

  it('persists false to localStorage', () => {
    const store = createStore()
    store.set(notificationSoundAtom, false)
    expect(store.get(notificationSoundAtom)).toBe(false)
    expect(mockStorage.getItem('mailrs_notification_sound')).toBe('false')
  })

  it('toggles between true and false', () => {
    const store = createStore()
    store.set(notificationSoundAtom, false)
    store.set(notificationSoundAtom, true)
    expect(store.get(notificationSoundAtom)).toBe(true)
  })
})

describe('settings module load behavior', () => {
  afterEach(() => {
    vi.resetModules()
    vi.unstubAllGlobals()
  })

  async function importFresh() {
    return await import('../settings')
  }

  it('reads stored notifications=false', async () => {
    vi.resetModules()
    const storage = makeLocalStorageMock()
    storage.setItem('mailrs_notifications', 'false')
    vi.stubGlobal('localStorage', storage)
    const { notificationsAtom: fresh } = await importFresh()
    const store = createStore()
    expect(store.get(fresh)).toBe(false)
  })

  it('defaults notification sound to true when nothing stored', async () => {
    vi.resetModules()
    vi.stubGlobal('localStorage', makeLocalStorageMock())
    const { notificationSoundAtom: fresh } = await importFresh()
    const store = createStore()
    expect(store.get(fresh)).toBe(true)
  })

  it('reads stored notification sound=false', async () => {
    vi.resetModules()
    const storage = makeLocalStorageMock()
    storage.setItem('mailrs_notification_sound', 'false')
    vi.stubGlobal('localStorage', storage)
    const { notificationSoundAtom: fresh } = await importFresh()
    const store = createStore()
    expect(store.get(fresh)).toBe(false)
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
