import type { MailListId } from '@/lib/mail-lists'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderHook, waitFor } from '@testing-library/react'
import { getDefaultStore } from 'jotai'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { useCurrentMailFilters } from '@/hooks/use-current-mail-filters'
import { useConversationsQuery } from '@/hooks/use-mail-queries'
import { MAIL_LIST_TABS, threadAxesOf } from '@/lib/mail-lists'
import { queryClient } from '@/lib/query-client'
import { mailListLoader } from '@/lib/route-loaders'
import { activeListAtom } from '@/store/ui'

vi.mock('@/store/auth', () => ({ getToken: () => 'test-token' }))

/**
 * The `/mail` route preload asks for the page the list on screen asks for,
 * and fills the cache line the list reads.
 *
 * It used to build its own path from `?folder=` / `?tab=` in the URL,
 * which nothing writes: every preload fetched the whole mailbox, a page no
 * list reads, and the list fetched its own page after mounting anyway.
 */
describe('mail list route preload', () => {
  let urls: string[] = []

  beforeEach(() => {
    urls = []
    queryClient.clear()
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        urls.push(String(url))
        return new Response(JSON.stringify({ items: [] }), {
          headers: { 'content-type': 'application/json' },
          status: 200,
        })
      })
    )
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    getDefaultStore().set(activeListAtom, 'inbox')
  })

  const tabs: MailListId[] = MAIL_LIST_TABS.filter((id) => threadAxesOf(id) !== null)

  it('covers every tab the conversation list serves', () => {
    expect(tabs).toContain('inbox')
    expect(tabs).toContain('quarantine')
    expect(tabs).not.toContain('send')
    expect(tabs).not.toContain('draft')
  })

  it.each(tabs)('%s: the preload asks for the URL the list asks for', async (tab) => {
    getDefaultStore().set(activeListAtom, tab)

    await mailListLoader({ request: new Request('http://mail.test/mail') })
    await waitFor(() => expect(urls.length).toBe(1))
    const preloaded = urls[0]

    // the list's own request, on a cache of its own
    urls = []
    const own = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    renderHook(() => useConversationsQuery(useCurrentMailFilters() ?? {}), {
      wrapper: ({ children }) => <QueryClientProvider client={own}>{children}</QueryClientProvider>,
    })
    await waitFor(() => expect(urls.length).toBe(1))
    expect(preloaded).toBe(urls[0])
  })

  it.each(tabs)('%s: the list finds the preloaded page and does not ask again', async (tab) => {
    getDefaultStore().set(activeListAtom, tab)

    await mailListLoader({ request: new Request('http://mail.test/mail') })
    await waitFor(() => expect(urls.length).toBe(1))

    const { result } = renderHook(() => useConversationsQuery(useCurrentMailFilters() ?? {}), {
      wrapper: ({ children }) => (
        <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
      ),
    })
    await waitFor(() => expect(result.current.isSuccess).toBe(true))
    expect(urls).toHaveLength(1)
  })
})
