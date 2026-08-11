import { beforeEach, describe, expect, it, vi } from 'vitest'

/**
 * Signing in must survive a full localStorage.
 *
 * On 2026-08-11 it did not: React Query's persister had written a
 * snapshot under a new key on every deploy and deleted none of them,
 * localStorage hit its quota, and `saveAuth`'s `setItem` threw. The
 * login POST answered 200 and the page said "Network error" — the
 * message its catch-all uses for anything unrecognised.
 */
describe('the session must not lose to a cache', () => {
  const AUTH = {
    accessible_domains: [],
    address: 'a@b.com',
    display_name: 'A',
    permissions: [],
    token: 't',
  }

  /**
   * A plain object, so `Object.keys` enumerates the stored keys the
   * way it does on a real `Storage`. A double carrying only methods
   * enumerates nothing, and the retry finds no caches to drop — the
   * first version of this test did exactly that and reported a
   * failure that was its own.
   */
  function storageThatIsFullUntilCachesGo(): Record<string, unknown> & {
    getItem: (k: string) => null | string
  } {
    const store: Record<string, unknown> = {
      'mailrs:rq:v1:old-build': 'x'.repeat(50),
      'mailrs:rq:v1:older-build': 'x'.repeat(50),
    }
    return Object.assign(store, {
      getItem: (k: string) => (typeof store[k] === 'string' ? (store[k] as string) : null),
      removeItem: (k: string) => {
        delete store[k]
      },
      setItem: (k: string, v: string) => {
        // Full while any cache blob is present — the shape the real
        // quota takes when the caches are what filled it.
        if (k === 'mailrs_auth' && Object.keys(store).some((x) => x.startsWith('mailrs:rq:'))) {
          throw new DOMException('The quota has been exceeded.', 'QuotaExceededError')
        }
        store[k] = v
      },
    })
  }

  beforeEach(() => {
    vi.resetModules()
  })

  it('drops the caches and retries rather than refusing the sign-in', async () => {
    const storage = storageThatIsFullUntilCachesGo()
    vi.stubGlobal('localStorage', storage)
    const { authAtom } = await import('../auth')
    const { createStore } = await import('jotai')
    const store = createStore()

    store.set(authAtom, AUTH)

    expect(storage.getItem('mailrs_auth')).toContain('"token":"t"')
    expect(storage.getItem('mailrs:rq:v1:old-build')).toBeNull()
  })

  /**
   * `loadAuth` runs at module scope. Throwing there takes the whole
   * app down before it renders, and a blob that will not parse is not
   * a session anyway.
   */
  it('a corrupt blob signs the reader out instead of crashing the app', async () => {
    const map = new Map<string, string>([['mailrs_auth', 'not json{']])
    vi.stubGlobal('localStorage', {
      length: 1,
      clear: () => map.clear(),
      getItem: (k: string) => map.get(k) ?? null,
      key: () => null,
      removeItem: (k: string) => void map.delete(k),
      setItem: (k: string, v: string) => void map.set(k, v),
    })
    const { getToken } = await import('../auth')
    expect(getToken()).toBeNull()
  })
})
