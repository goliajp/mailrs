import type { ConversationSummary } from '@/lib/types'
import type { WireSentMessage } from '@/wire/schemas/mail'
import type { ReactNode } from 'react'

import { renderHook } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { useCurrentSelection } from '@/hooks/use-current-list'
import { activeListAtom, pickedItemAtom, selectMailListAtom } from '@/store/ui'

// The four sources, stubbed at the query boundary — the same shape
// `list.test.tsx` uses. Set the fields before rendering.
const stub: {
  conversations: ConversationSummary[]
  drafts: unknown[]
  sends: unknown[]
  sent: WireSentMessage[]
} = { conversations: [], drafts: [], sends: [], sent: [] }

vi.mock('@/hooks/use-flat-conversations', () => ({
  useFlatConversations: () => ({
    conversations: stub.conversations,
    hasMore: false,
    initialLoading: false,
    loadingMore: false,
    loadMore: async () => {},
    refresh: async () => {},
  }),
}))
vi.mock('@/hooks/use-sent-messages', () => ({
  useSentMessagesQuery: () => ({ data: stub.sent, isLoading: false }),
}))
vi.mock('@/hooks/use-sends', () => ({
  useSendsQuery: () => ({ data: stub.sends, isLoading: false }),
}))
vi.mock('@/hooks/use-drafts', () => ({
  useDraftsQuery: () => ({ data: stub.drafts, isLoading: false }),
}))
vi.mock('@/hooks/use-mail-queries', () => ({
  useThreadQuery: () => ({ data: [] }),
}))

function conversation(id: string, date: number): ConversationSummary {
  return {
    archived: false,
    category: 'inbox',
    count: 1,
    flagged: false,
    importance_level: 'normal',
    last_date: date,
    participants: [],
    pinned: false,
    sent_count: 0,
    snippet: '',
    subject: id,
    thread_id: id,
    unread_count: 0,
  } as unknown as ConversationSummary
}

function sentMessage(threadId: string, uid: number, date: number): WireSentMessage {
  return {
    internal_date: date,
    message_id: `<${threadId}@x>`,
    subject: threadId,
    thread_id: threadId,
    to: 'a@b.c',
    uid,
  } as unknown as WireSentMessage
}

beforeEach(() => {
  stub.conversations = []
  stub.drafts = []
  stub.sends = []
  stub.sent = []
})
afterEach(() => vi.clearAllMocks())

function wrapper(store: ReturnType<typeof createStore>) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <Provider store={store}>{children}</Provider>
  }
}

/**
 * Every list opens its first row when you arrive at it. This is the
 * property `resolveSelection` exists for, asserted through the hook the
 * reading pane actually calls rather than through the pure function —
 * a source whose rows never reach `useCurrentListRows` passes the unit
 * test and still shows an empty pane.
 */
describe('useCurrentSelection — arriving at a list', () => {
  it('opens the first conversation of a thread list', () => {
    stub.conversations = [conversation('t-new', 200), conversation('t-old', 100)]
    const store = createStore()
    store.set(activeListAtom, 'archived')

    const { result } = renderHook(() => useCurrentSelection(), { wrapper: wrapper(store) })
    expect(result.current).toEqual({ threadId: 't-new', uid: null })
  })

  it('opens the first row of Send, message and all', () => {
    stub.sent = [sentMessage('t-a', 7, 200), sentMessage('t-b', 3, 100)]
    const store = createStore()
    store.set(activeListAtom, 'send')

    const { result } = renderHook(() => useCurrentSelection(), { wrapper: wrapper(store) })
    expect(result.current).toEqual({ threadId: 't-a', uid: 7 })
  })

  it('opens nothing on Draft — a row there would pop the composer', () => {
    stub.drafts = [{ body: '', created_at: 1, id: 1, subject: 's', to: 'a@b.c', updated_at: 1 }]
    const store = createStore()
    store.set(activeListAtom, 'draft')

    const { result } = renderHook(() => useCurrentSelection(), { wrapper: wrapper(store) })
    expect(result.current).toBeNull()
  })

  /**
   * The pick is scoped to the list it was made in: a thread you replied
   * in is in both Inbox and Send, so a pick carried across would look
   * valid and the pane would keep showing the list you just left.
   */
  it('drops a pick made in another list and takes the new list first row', () => {
    stub.conversations = [conversation('t-arch', 200)]
    stub.sent = [sentMessage('t-sent', 7, 200)]
    const store = createStore()
    store.set(activeListAtom, 'send')
    store.set(pickedItemAtom, { list: 'send', threadId: 't-sent', uid: 7 })
    store.set(selectMailListAtom, 'archived')

    const { result } = renderHook(() => useCurrentSelection(), { wrapper: wrapper(store) })
    expect(result.current).toEqual({ threadId: 't-arch', uid: null })
  })

  it('has nothing to open when the list is empty', () => {
    const store = createStore()
    store.set(activeListAtom, 'archived')

    const { result } = renderHook(() => useCurrentSelection(), { wrapper: wrapper(store) })
    expect(result.current).toBeNull()
  })
})
