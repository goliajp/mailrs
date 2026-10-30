// The unread badge is the server's count of unread conversations. It
// used to add up the loaded rows of the Unread list, so past the first
// page it counted short.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, renderHook, waitFor } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, describe, expect, it, vi } from 'vitest'

const served = { count: 137, paths: [] as string[] }
vi.mock('@/wire/client', () => ({
  wireFetch: (_schema: unknown, { path }: { path: string }) => {
    served.paths.push(path)
    return Promise.resolve({ count: served.count })
  },
}))
// one loaded page holding a single unread conversation
vi.mock('@/hooks/use-flat-conversations', () => ({
  useFlatConversations: () => ({ conversations: [{ thread_id: 't1', unread_count: 1 }] }),
}))

import { useCurrentUnreadCount } from '@/hooks/use-current-mail-filters'
import { conversationKeys } from '@/store/query-keys-v21'

afterEach(cleanup)

describe('unread badge', () => {
  it('shows the mailbox count and follows a refresh of the lists', async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const { result } = renderHook(() => useCurrentUnreadCount(), {
      wrapper: ({ children }) => (
        <QueryClientProvider client={client}>
          <Provider store={createStore()}>{children}</Provider>
        </QueryClientProvider>
      ),
    })
    await waitFor(() => expect(result.current).toBe(137))
    expect(served.paths).toEqual(['/conversations/unseen-count'])

    served.count = 136
    await act(() => client.invalidateQueries({ queryKey: conversationKeys.lists() }))
    await waitFor(() => expect(result.current).toBe(136))
  })
})
