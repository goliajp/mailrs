import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

/**
 * Boolean query parameters go out as `true`.
 *
 * The server reads them with axum's `Query`, which takes `true` and
 * `false` and refuses `1` — the whole request, not only the parameter
 * (`request_contract_search.rs::a_one_is_not_a_boolean`). A list read
 * that sent `unread=1` failed every time it was made.
 */
describe('boolean query parameters', () => {
  let urls: string[] = []

  beforeEach(() => {
    vi.resetModules()
    urls = []
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        urls.push(url)
        return new Response(JSON.stringify({ items: [] }), {
          headers: { 'content-type': 'application/json' },
          status: 200,
        })
      })
    )
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  function flags(url: string): Record<string, string> {
    const params = new URL(url, 'http://mail.test').searchParams
    params.delete('limit')
    params.delete('label')
    return Object.fromEntries(params)
  }

  it('fetchConversationList', async () => {
    const { fetchConversationList } = await import('../endpoints/conversations')
    await fetchConversationList({
      archived: true,
      includeQuarantined: true,
      label: '3',
      starred: true,
      trashed: true,
      unread: true,
    })
    expect(flags(urls[0])).toEqual({
      archived: 'true',
      include_quarantined: 'true',
      starred: 'true',
      trashed: 'true',
      unread: 'true',
    })
  })

  it('the mail list route loader', async () => {
    const { mailListLoader } = await import('@/lib/route-loaders')
    await mailListLoader({ request: new Request('http://mail.test/mail?tab=unread') })
    await vi.waitFor(() => expect(urls.some((u) => u.includes('/conversations?'))).toBe(true))
    const list = urls.find((u) => u.includes('/conversations?')) ?? ''
    expect(flags(list)).toEqual({ unread: 'true' })
  })
})
