import { beforeEach, describe, expect, it, vi } from 'vitest'

/**
 * The persister used to key on the build id, so every deploy wrote a
 * new blob and deleted none. A browser that had seen many releases
 * held one full query-cache snapshot per release until the origin ran
 * out of storage — and then signing in failed, because the session
 * write is a `localStorage.setItem` like any other.
 */
describe('dropOrphanedCaches', () => {
  beforeEach(() => {
    vi.resetModules()
  })

  it('removes every earlier build snapshot and keeps the current one', async () => {
    const map = new Map<string, string>([
      ['mailrs:rq:v1', 'current'],
      ['mailrs:rq:v1:abc123', 'a release ago'],
      ['mailrs:rq:v1:def456', 'two releases ago'],
      ['mailrs_auth', 'the session'],
      ['mailrs_saved_email', 'someone@example.com'],
    ])
    // A plain object, so `Object.keys` enumerates the stored keys the
    // way it does on a real `Storage`. A mock with only methods
    // enumerates nothing, and the sweep would look like it worked
    // while removing nothing at all.
    const storage = Object.assign(Object.fromEntries(map), {
      getItem: (k: string) => map.get(k) ?? null,
      removeItem: (k: string) => {
        map.delete(k)
        delete (storage as Record<string, unknown>)[k]
      },
      setItem: (k: string, v: string) => {
        map.set(k, v)
        ;(storage as Record<string, unknown>)[k] = v
      },
    })
    vi.stubGlobal('window', { localStorage: storage })

    const { dropOrphanedCaches } = await import('../query-client')
    expect(dropOrphanedCaches()).toBe(2)
    expect([...map.keys()].sort()).toEqual(['mailrs:rq:v1', 'mailrs_auth', 'mailrs_saved_email'])
  })
})
