import type { WireSentMessage } from '@/wire/schemas/mail'

import { cleanup, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, describe, expect, it, vi } from 'vitest'

// The list reads one hook; stubbing it keeps this a render-shape test
// rather than a React Query integration test. Same approach as
// conversation-list.test.tsx.
const sentStub: { messages: WireSentMessage[] } = { messages: [] }

vi.mock('@/hooks/use-sent-messages', () => ({
  useSentMessagesQuery: () => ({ data: sentStub.messages, isLoading: false }),
}))

vi.mock('@/hooks/use-mail-mutations', () => ({
  useDeleteMutation: () => ({ mutate: vi.fn() }),
}))

// FilterBar pulls the router and the whole filter store; the rows are
// what this file is about.
vi.mock('@/components/conversation-list-filter-bar', () => ({
  FilterBar: () => null,
}))

import { SentList } from '@/components/sent-list'

function msg(over: Partial<WireSentMessage> & { message_id: string }): WireSentMessage {
  return {
    internal_date: 1785369273,
    subject: 'send test',
    thread_id: over.message_id,
    to: 'GOLIA <goliaaccess@gmail.com>',
    // 0 is what `applyOptimisticSent` writes, and the whole point.
    uid: 0,
    ...over,
  }
}

function renderList() {
  return render(
    <Provider store={createStore()}>
      <SentList />
    </Provider>
  )
}

afterEach(() => {
  cleanup()
  sentStub.messages = []
})

describe('SentList row identity', () => {
  /// Two optimistic placeholders both carry `uid: 0`, so keying rows on
  /// uid gave them the same React key and React kept the stale node —
  /// sending two mails showed three rows until a refresh rebuilt the
  /// tree (2026-07-30). `message_id` is unique per send, is never 0, and
  /// is the identity `dedupeSentByMessageId` already dedupes on.
  it('renders one row per message even when every uid is the placeholder 0', () => {
    sentStub.messages = [
      msg({ message_id: '4974a5fd975d0dab@golia.jp', subject: 'send test 2' }),
      msg({ message_id: '3d6af3cd9ece8e18@golia.jp', subject: 'send test 1' }),
    ]
    renderList()

    expect(screen.getAllByRole('listitem')).toHaveLength(2)
    expect(screen.getByText('send test 1')).toBeTruthy()
    expect(screen.getByText('send test 2')).toBeTruthy()
  })

  /// The real rows the server returns have distinct uids, so this case
  /// passed before the fix too. It is here so a regression that keys on
  /// something unique-but-wrong (an index, say) still has to survive a
  /// mixed list.
  it('renders a mix of placeholder and server rows', () => {
    sentStub.messages = [
      msg({ message_id: 'fresh@golia.jp', subject: 'just sent', uid: 0 }),
      msg({ message_id: 'older@golia.jp', subject: 'already stored', uid: 30744 }),
    ]
    renderList()

    expect(screen.getAllByRole('listitem')).toHaveLength(2)
  })

  /// The actual reproduction, which a single render cannot show.
  ///
  /// Duplicate keys inside one render only make React warn — both
  /// children still mount. The stale node appears on the *next*
  /// reconciliation, which is the sequence a second send produces:
  ///
  ///   1. send A → cache [A(uid 0)]
  ///   2. send B → cache [B(uid 0), A(uid 0)]   ← two rows, same key
  ///   3. refetch lands → [B(uid 30745), A(uid 30744)]
  ///
  /// Keying on uid makes step 2 ambiguous, and step 3 reconciles against
  /// it. Keying on message_id keeps every step distinct.
  it('survives the second-send sequence without leaving a stale row', () => {
    sentStub.messages = [msg({ message_id: 'a@golia.jp', subject: 'send test 1' })]
    const { rerender } = renderList()
    expect(screen.getAllByRole('listitem')).toHaveLength(1)

    // Step 2: both are optimistic placeholders, so both carry uid 0.
    sentStub.messages = [
      msg({ message_id: 'b@golia.jp', subject: 'send test 2' }),
      msg({ message_id: 'a@golia.jp', subject: 'send test 1' }),
    ]
    rerender(
      <Provider store={createStore()}>
        <SentList />
      </Provider>
    )
    expect(screen.getAllByRole('listitem')).toHaveLength(2)

    // Step 3: the refetch replaces both with server rows.
    sentStub.messages = [
      msg({ message_id: 'b@golia.jp', subject: 'send test 2', uid: 30745 }),
      msg({ message_id: 'a@golia.jp', subject: 'send test 1', uid: 30744 }),
    ]
    rerender(
      <Provider store={createStore()}>
        <SentList />
      </Provider>
    )
    expect(screen.getAllByRole('listitem'), 'two sends must never render three rows').toHaveLength(
      2
    )
  })

  it('says so when there is nothing sent', () => {
    renderList()
    expect(screen.getByText('No sent messages')).toBeTruthy()
  })
})
