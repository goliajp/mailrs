/**
 * One set of compose behaviours, run against every shell: the inline reply,
 * the page and a floating window. Sending, undoing a send, saving and
 * restoring a draft, a failed send that keeps the draft, and scheduling
 * must not depend on which frame the composer is in.
 */
import type { ComposerStart } from '../start'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { MemoryRouter } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { NewConversation } from '@/components/new-conversation'
import { ReplyBox } from '@/components/reply-box'
import { authAtom } from '@/store/auth'
import { composerWindowsAtom } from '@/store/composer-windows'
import { composeDraftSourceAtom, composingNewAtom } from '@/store/ui'

import { FloatingComposers } from '../floating-composers'

const net = vi.hoisted(() => ({
  deleteDraft: vi.fn(),
  deletePending: vi.fn(),
  listDrafts: vi.fn(),
  saveDraft: vi.fn(),
  sendMail: vi.fn(),
  toastError: vi.fn(),
  toastInfo: vi.fn(),
  toastSuccess: vi.fn(),
}))

vi.mock('@/lib/api', async (original) => ({
  ...(await original<typeof import('@/lib/api')>()),
  deleteDraft: net.deleteDraft,
  listDrafts: net.listDrafts,
  saveDraft: net.saveDraft,
}))
vi.mock('@/lib/send-mail', async (original) => ({
  ...(await original<typeof import('@/lib/send-mail')>()),
  sendMail: net.sendMail,
}))
vi.mock('@/wire/endpoints/mail', async (original) => ({
  ...(await original<typeof import('@/wire/endpoints/mail')>()),
  wireDeletePendingSend: net.deletePending,
}))
vi.mock('@goliapkg/gds', async (original) => ({
  ...(await original<typeof import('@goliapkg/gds')>()),
  toast: { error: net.toastError, info: net.toastInfo, success: net.toastSuccess },
}))

const DRAFT = {
  bcc: '',
  body: 'saved earlier',
  cc: '',
  created_at: null,
  from: 'alias@example.com',
  id: 7,
  reply_to_thread_id: 'thread-1',
  // a whole minute: the schedule field holds minutes
  scheduled_at: 1_899_999_960,
  subject: 'Re: Test Subject',
  to: 'alice@example.com',
  updated_at: null,
}

const CTX = {
  lastMessageId: 'msg-1',
  originalBody: 'Original body',
  originalDate: '2024-01-01 10:00',
  originalFrom: 'alice@example.com',
  replyAllRecipients: 'alice@example.com, bob@example.com',
  replyRecipients: 'alice@example.com',
  subject: 'Test Subject',
  threadId: 'thread-1',
}

type Shell = {
  /** Mounts the shell, from a saved draft when `fromDraft`. */
  mount: (store: ReturnType<typeof createStore>, fromDraft: boolean) => React.ReactNode
  name: string
  /** The address a sent message goes to. */
  recipient: string
}

const SHELLS: Shell[] = [
  {
    name: 'inline reply',
    recipient: 'alice@example.com',
    mount: () => <ReplyBox {...CTX} mode="reply" onModeChange={() => {}} onSent={() => {}} />,
  },
  {
    name: 'page',
    recipient: 'carol@example.com',
    mount: (store, fromDraft) => {
      store.set(composingNewAtom, true)
      if (fromDraft)
        store.set(composeDraftSourceAtom, {
          ...DRAFT,
          replyToThreadId: 'thread-1',
          scheduledAt: DRAFT.scheduled_at,
        })
      return <NewConversation />
    },
  },
  {
    name: 'floating window',
    recipient: 'carol@example.com',
    mount: (store, fromDraft) => {
      store.set(composerWindowsAtom, [
        { id: 'w1', minimized: false, start: windowStart(fromDraft) },
      ])
      return <FloatingComposers />
    },
  },
]

async function body(): Promise<HTMLTextAreaElement> {
  return waitFor(() => {
    const el = document.querySelector('textarea')
    if (!el) throw new Error('editor not mounted yet')
    return el
  })
}

function clickSend() {
  fireEvent.click(screen.getByRole('button', { name: /^send$/i }))
}

function setup(shell: Shell, fromDraft = false) {
  const store = createStore()
  store.set(authAtom, {
    accessible_domains: [],
    address: 'user@example.com',
    display_name: 'U',
    is_admin: false,
    is_super: false,
    permissions: [],
  })
  const ui = shell.mount(store, fromDraft)
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <Provider store={store}>
        <MemoryRouter>{ui}</MemoryRouter>
      </Provider>
    </QueryClientProvider>
  )
}

async function write(shell: Shell, text: string) {
  const to = screen.queryByLabelText('To') as HTMLInputElement | null
  if (to && !to.value) fireEvent.change(to, { target: { value: shell.recipient } })
  fireEvent.change(await body(), { target: { value: text } })
}

beforeEach(() => {
  net.listDrafts.mockResolvedValue([])
  net.saveDraft.mockResolvedValue({ id: 41, success: true })
  net.deleteDraft.mockResolvedValue({ success: true })
  net.sendMail.mockResolvedValue({ message_id: 'mid-1', success: true })
  net.deletePending.mockResolvedValue(undefined)
})

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe.each(SHELLS)('the composer in the $name', (shell) => {
  it('sends what was written to whom it was addressed', async () => {
    setup(shell)
    await write(shell, 'hello there')
    clickSend()
    await waitFor(() => expect(net.sendMail).toHaveBeenCalledTimes(1))
    const sent = net.sendMail.mock.calls[0][0]
    expect(sent.to).toEqual([shell.recipient])
    expect(sent.body).toContain('hello there')
    expect(sent.from).toBe('user@example.com')
    expect(sent.scheduledAt).toBeNull()
  })

  it('takes a send back from the toast while the server still holds it', async () => {
    setup(shell)
    await write(shell, 'oops')
    clickSend()
    await waitFor(() => expect(net.toastSuccess).toHaveBeenCalled())
    const [, options] = net.toastSuccess.mock.calls[0]
    await options.action.onClick()
    expect(net.deletePending).toHaveBeenCalledWith('mid-1')
  })

  it('saves what is typed as a draft', async () => {
    setup(shell)
    await write(shell, 'a draft in progress')
    // autosave runs on a 3 s interval, so the first save can be 3 s away
    await waitFor(
      () =>
        expect(net.saveDraft).toHaveBeenCalledWith(
          expect.objectContaining({ body: 'a draft in progress' })
        ),
      { timeout: 8_000 }
    )
  })

  it('comes back to a saved draft', async () => {
    net.listDrafts.mockResolvedValue([DRAFT])
    setup(shell, true)
    const editor = await body()
    await waitFor(() => expect(editor.value).toBe('saved earlier'))
  })

  it('comes back to who a saved draft is from and when it goes', async () => {
    net.listDrafts.mockResolvedValue([DRAFT])
    setup(shell, true)
    const editor = await body()
    await waitFor(() => expect(editor.value).toBe('saved earlier'))
    clickSend()
    await waitFor(() => expect(net.sendMail).toHaveBeenCalledTimes(1))
    const sent = net.sendMail.mock.calls[0][0]
    expect(sent.from).toBe('alias@example.com')
    expect(sent.scheduledAt).toBe(1_899_999_960)
  })

  it('keeps the draft when the send fails, and offers to try again', async () => {
    net.sendMail.mockRejectedValue(new Error('offline'))
    setup(shell)
    await write(shell, 'not lost')
    clickSend()
    await waitFor(() => expect(net.toastError).toHaveBeenCalled())
    await waitFor(() =>
      expect(net.saveDraft).toHaveBeenCalledWith(expect.objectContaining({ body: 'not lost' }))
    )
    const [, options] = net.toastError.mock.calls[0]
    expect(options.action.label).toBe('Retry')
  })

  it('schedules a send', async () => {
    setup(shell)
    await write(shell, 'later')
    fireEvent.click(screen.getByRole('button', { name: 'Schedule send' }))
    fireEvent.change(screen.getByLabelText('Schedule send time'), {
      target: { value: '2030-01-02T09:30' },
    })
    clickSend()
    await waitFor(() => expect(net.sendMail).toHaveBeenCalledTimes(1))
    expect(net.sendMail.mock.calls[0][0].scheduledAt).toBe(
      Math.floor(new Date('2030-01-02T09:30').getTime() / 1000)
    )
  })
})

function windowStart(fromDraft: boolean): ComposerStart {
  if (fromDraft)
    return {
      draft: { ...DRAFT, replyToThreadId: 'thread-1', scheduledAt: DRAFT.scheduled_at },
      kind: 'draft',
    }
  return { kind: 'new' }
}
