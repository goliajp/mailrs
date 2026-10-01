import type { WireSentMessage } from '@/wire/schemas/mail'
import type { WireSend } from '@/wire/schemas/sends'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, describe, expect, it, vi } from 'vitest'

// The list reads two hooks; stubbing them keeps this a render-shape test
// rather than a React Query integration test.
const stub: { messages: WireSentMessage[]; sends: WireSend[] } = { messages: [], sends: [] }

vi.mock('@/hooks/use-sent-messages', () => ({
  useSentMessagesQuery: () => ({ data: stub.messages, isLoading: false }),
}))

const cancelCalls: string[] = []
const deleteCalls: string[] = []
vi.mock('@/hooks/use-sends', () => ({
  useCancelSendMutation: () => ({
    isPending: false,
    mutate: (id: string) => cancelCalls.push(id),
  }),
  useDeleteSendMutation: () => ({
    isPending: false,
    mutate: (id: string) => deleteCalls.push(id),
  }),
  useResendMutation: () => ({ isPending: false, mutate: vi.fn() }),
  useSendsQuery: () => ({ data: stub.sends }),
}))

// FilterBar pulls the router and the whole filter store; the rows are what
// this file is about.
vi.mock('@/components/conversation-list-filter-bar', () => ({
  FilterBar: () => null,
}))

import { SendList } from '../send-list'

// `useCurrentListRows` calls every source hook so the hook order stays
// fixed as the list changes (only one of them is enabled), so a
// QueryClient has to be in scope even though this file mocks the send
// queries it cares about.
const testQueryClient = new QueryClient({
  defaultOptions: { queries: { gcTime: 0, retry: false } },
})

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
    <QueryClientProvider client={testQueryClient}>
      <Provider store={createStore()}>
        <SendList />
      </Provider>
    </QueryClientProvider>
  )
}

function send(over: Partial<WireSend> & { send_id: string }): WireSend {
  return {
    can_resend: true,
    created_at: 1785369273,
    recipients: [],
    resent_from: null,
    status: 'delivered',
    subject: 'send test',
    thread_id: over.send_id,
    to: ['GOLIA <goliaaccess@gmail.com>'],
    ...over,
  }
}

afterEach(() => {
  cleanup()
  stub.messages = []
  stub.sends = []
})

describe('SendList row identity', () => {
  /**
   * The reproduction a single render cannot show. Duplicate keys inside
   * one render only make React warn — both children still mount. The
   * stale node appears on the *next* reconciliation, which is the
   * sequence a second send produces:
   *
   *   1. send A → cache [A(uid 0)]
   *   2. send B → cache [B(uid 0), A(uid 0)]   ← two rows, same uid
   *   3. refetch lands → [B(uid 30745), A(uid 30744)]
   *
   * Keying on uid makes step 2 ambiguous and step 3 reconciles against
   * it; two sends rendered three rows on 2026-07-30. `message_id` keeps
   * every step distinct.
   */
  it('survives the second-send sequence without leaving a stale row', () => {
    stub.messages = [msg({ message_id: 'a@golia.jp', subject: 'send test 1' })]
    const { rerender } = renderList()
    expect(screen.getAllByRole('listitem')).toHaveLength(1)

    stub.messages = [
      msg({ message_id: 'b@golia.jp', subject: 'send test 2' }),
      msg({ message_id: 'a@golia.jp', subject: 'send test 1' }),
    ]
    rerender(
      <QueryClientProvider client={testQueryClient}>
        <Provider store={createStore()}>
          <SendList />
        </Provider>
      </QueryClientProvider>
    )
    expect(screen.getAllByRole('listitem')).toHaveLength(2)

    stub.messages = [
      msg({ message_id: 'b@golia.jp', subject: 'send test 2', uid: 30745 }),
      msg({ message_id: 'a@golia.jp', subject: 'send test 1', uid: 30744 }),
    ]
    rerender(
      <QueryClientProvider client={testQueryClient}>
        <Provider store={createStore()}>
          <SendList />
        </Provider>
      </QueryClientProvider>
    )
    expect(screen.getAllByRole('listitem'), 'two sends must never render three rows').toHaveLength(
      2
    )
  })

  it('says so when there is nothing sent', () => {
    renderList()
    expect(screen.getByText('Nothing sent yet')).toBeTruthy()
  })
})

// The status filter renders a chip for every status name, so a
// document-wide text query matches the chip as well as the badge. Every
// assertion about a badge is scoped to the row it belongs to.
const STATUS_LABELS = ['Delivered', 'Failed', 'Sending', 'Scheduled', 'Partly delivered']

function badgesInRow(): string[] {
  const row = screen.getByRole('listitem')
  return STATUS_LABELS.filter((label) => within(row).queryAllByText(label).length > 0)
}

describe('SendList status', () => {
  /// The reason the view exists: a failed send has to be visibly failed.
  it('shows a badge for a send that has a record', () => {
    stub.messages = [msg({ message_id: 'a@golia.jp' })]
    stub.sends = [send({ send_id: 'a@golia.jp', status: 'failed' })]
    renderList()
    expect(badgesInRow()).toEqual(['Failed'])
    expect(screen.getByText('1 needs attention')).toBeTruthy()
  })

  /// Every send before the projection shipped has no record. It still has
  /// to appear, and with no badge — an "unknown" pill on hundreds of rows
  /// would be noise, and a "delivered" one would be a false claim.
  it('shows a send with no record, and no badge', () => {
    stub.messages = [msg({ message_id: 'ancient@golia.jp', subject: 'from last year' })]
    renderList()
    expect(screen.getByText('from last year')).toBeTruthy()
    expect(badgesInRow()).toEqual([])
    expect(screen.queryByText(/needs? attention/)).toBeNull()
  })

  /// The reported bug (2026-07-30): a delivered reply with a Send row but
  /// no sent-axis entry rendered nowhere, because the list was built by
  /// mapping over the axis. Nothing on the ingest path writes that axis.
  it('renders a send that only the projection knows about', () => {
    stub.sends = [
      send({
        send_id: '9d8549f828cd6aea@golia.jp',
        status: 'delivered',
        subject: 'Re: 決算について',
        to: ['nagata@nagatax.tokyo.jp'],
      }),
    ]
    renderList()
    expect(screen.getAllByRole('listitem')).toHaveLength(1)
    expect(screen.getByText('Re: 決算について')).toBeTruthy()
    expect(badgesInRow()).toEqual(['Delivered'])
  })

  /// A delivered send needs no counter — the badge already says it landed.
  it('does not count a delivered send as needing attention', () => {
    stub.messages = [msg({ message_id: 'a@golia.jp' })]
    stub.sends = [send({ send_id: 'a@golia.jp', status: 'delivered' })]
    renderList()
    expect(badgesInRow()).toEqual(['Delivered'])
    expect(screen.queryByText(/needs? attention/)).toBeNull()
  })
})

describe('cancelling a send', () => {
  afterEach(() => {
    cancelCalls.length = 0
  })

  /**
   * The control exists only where there is something to stop.
   *
   * On a delivered row it would promise a recall, which nothing here
   * can do — the message is with the receiving server. Three
   * invitations sat in the queue for a day with no way to stop them,
   * which is why the button exists; offering it one row too far is how
   * it would start lying.
   */
  it('offers cancel on a send in flight and not on one that landed', () => {
    stub.sends = [
      send({ send_id: 'a@golia.jp', status: 'sending' }),
      send({ send_id: 'b@golia.jp', status: 'delivered' }),
    ]
    stub.messages = [msg({ message_id: 'a@golia.jp' }), msg({ message_id: 'b@golia.jp' })]
    renderList()

    const buttons = screen.getAllByLabelText('Cancel this send')
    expect(buttons).toHaveLength(1)
  })

  /** And a scheduled one, which has not been attempted at all. */
  it('offers cancel on a scheduled send', () => {
    stub.sends = [send({ send_id: 'a@golia.jp', status: 'scheduled' })]
    stub.messages = [msg({ message_id: 'a@golia.jp' })]
    renderList()
    expect(screen.getAllByLabelText('Cancel this send')).toHaveLength(1)
  })

  /** Pressing it asks about that send and no other. */
  it('cancels the row it was pressed on', () => {
    stub.sends = [
      send({ send_id: 'a@golia.jp', status: 'sending' }),
      send({ send_id: 'b@golia.jp', status: 'sending' }),
    ]
    stub.messages = [msg({ message_id: 'a@golia.jp' }), msg({ message_id: 'b@golia.jp' })]
    renderList()

    const buttons = screen.getAllByLabelText('Cancel this send')
    expect(buttons).toHaveLength(2)
    fireEvent.click(buttons[1] as HTMLElement)
    expect(cancelCalls).toHaveLength(1)
  })
})

describe('SendList delete', () => {
  afterEach(() => {
    deleteCalls.length = 0
  })

  // a failed send that the sender has given up on has to be able to leave
  // the list; before this the only way out was deleting the conversation,
  // which left the send record behind
  it('deletes the failed send it was opened on', () => {
    stub.sends = [
      send({ send_id: 'a@golia.jp', status: 'delivered' }),
      send({
        recipients: [
          {
            code: 550,
            delivered: false,
            message: '5.7.1 blocked',
            pending: false,
            recipient: 'x@hotmail.com',
          },
        ],
        send_id: 'b@golia.jp#r1',
        status: 'failed',
      }),
    ]
    stub.messages = [msg({ message_id: 'a@golia.jp' }), msg({ message_id: 'b@golia.jp' })]
    renderList()

    expect(screen.queryByText('Delete')).toBeNull()
    const failedRow = screen
      .getAllByRole('listitem')
      .find((item) => item.textContent?.includes('Failed'))
    fireEvent.click(failedRow?.querySelector('button') as HTMLElement)
    fireEvent.click(screen.getByText('Delete'))
    expect(deleteCalls).toEqual(['b@golia.jp#r1'])
  })
})
