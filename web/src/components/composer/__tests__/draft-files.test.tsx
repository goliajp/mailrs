/**
 * A file added in the composer is uploaded at once and kept with the draft:
 * every autosave names it, a reopened draft lists it, and a send names the
 * draft so the server attaches it instead of the form carrying it again.
 */
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { MemoryRouter } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { NewConversation } from '@/components/new-conversation'
import { ReplyBox } from '@/components/reply-box'
import { authAtom } from '@/store/auth'
import { composeDraftSourceAtom, composingNewAtom } from '@/store/ui'
import { WireErrorException } from '@/wire/errors'

const net = vi.hoisted(() => ({
  deleteDraft: vi.fn(),
  listDrafts: vi.fn(),
  saveDraft: vi.fn(),
  sendMail: vi.fn(),
  upload: vi.fn(),
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
vi.mock('@/wire/endpoints/draft-attachments', async (original) => ({
  ...(await original<typeof import('@/wire/endpoints/draft-attachments')>()),
  wireUploadDraftAttachment: net.upload,
}))

const QUOTE = {
  blob_id: 'U3a6eb0790f39ac87c94f3856b2dd2c5d110e6811602261a9a923d3bb23adc8b7',
  content_type: 'application/pdf',
  name: 'quote.pdf',
  size: 48213,
}

const NOTES = {
  blob_id: 'U0000000000000000000000000000000000000000000000000000000000000001',
  content_type: 'text/plain',
  name: 'notes.txt',
  size: 5,
}

const DRAFT = {
  attachments: [QUOTE],
  bcc: '',
  body: 'the quote is attached',
  cc: '',
  created_at: null,
  from: '',
  id: 7,
  reply_to_thread_id: 'thread-1',
  scheduled_at: null,
  subject: 'Re: Quote',
  to: 'alice@example.com',
  updated_at: null,
}

async function attach(file: File) {
  await editor()
  const input = document.querySelector('input[type="file"]') as HTMLInputElement
  fireEvent.change(input, { target: { files: [file] } })
}

function clickSend() {
  fireEvent.click(screen.getByRole('button', { name: /^send$/i }))
}

async function editor(): Promise<HTMLTextAreaElement> {
  return waitFor(() => {
    const el = document.querySelector('textarea')
    if (!el) throw new Error('editor not mounted yet')
    return el
  })
}

function mount(store: ReturnType<typeof createStore>, ui: React.ReactNode) {
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

function openPage(fromDraft: boolean) {
  const store = createStore()
  signIn(store)
  store.set(composingNewAtom, true)
  if (fromDraft)
    store.set(composeDraftSourceAtom, {
      attachments: DRAFT.attachments,
      bcc: '',
      body: DRAFT.body,
      cc: '',
      from: '',
      id: DRAFT.id,
      replyToThreadId: DRAFT.reply_to_thread_id,
      scheduledAt: null,
      subject: DRAFT.subject,
      to: DRAFT.to,
    })
  mount(store, <NewConversation />)
}

function signIn(store: ReturnType<typeof createStore>) {
  store.set(authAtom, {
    accessible_domains: [],
    address: 'user@example.com',
    display_name: 'U',
    is_admin: false,
    is_super: false,
    permissions: [],
  })
}

beforeEach(() => {
  net.listDrafts.mockResolvedValue([])
  net.saveDraft.mockResolvedValue({ id: 41, success: true })
  net.deleteDraft.mockResolvedValue({ success: true })
  net.sendMail.mockResolvedValue({ message_id: 'mid-1', success: true })
})

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('files kept with a draft', () => {
  it('uploads a file as it is added and saves it with the draft', async () => {
    net.upload.mockResolvedValue(NOTES)
    openPage(false)
    fireEvent.change(screen.getByLabelText('To'), { target: { value: 'carol@example.com' } })
    fireEvent.change(await editor(), { target: { value: 'notes attached' } })
    const file = new File(['hello'], 'notes.txt', { type: 'text/plain' })
    await attach(file)
    expect(net.upload).toHaveBeenCalledWith(file)
    await waitFor(
      () =>
        expect(net.saveDraft).toHaveBeenCalledWith(
          expect.objectContaining({ attachments: [NOTES], body: 'notes attached' })
        ),
      { timeout: 8_000 }
    )
  })

  it('a reopened draft lists its files, each opening through the draft file route', async () => {
    openPage(true)
    const link = await screen.findByRole('link', { name: /quote\.pdf/ })
    expect(link.getAttribute('href')).toBe(
      `/api/mail/draft-attachments/${QUOTE.blob_id}?name=quote.pdf&type=application%2Fpdf`
    )
    expect(screen.getByText('47KB')).toBeTruthy()
  })

  it('a file removed from a reopened draft is saved as removed', async () => {
    openPage(true)
    fireEvent.click(await screen.findByRole('button', { name: 'Remove quote.pdf' }))
    expect(screen.queryByRole('link', { name: /quote\.pdf/ })).toBeNull()
    await waitFor(
      () =>
        expect(net.saveDraft).toHaveBeenCalledWith(
          expect.objectContaining({ attachments: [], id: 7 })
        ),
      { timeout: 8_000 }
    )
  })

  it('the inline reply comes back to its draft with the files', async () => {
    net.listDrafts.mockResolvedValue([DRAFT])
    const store = createStore()
    signIn(store)
    mount(
      store,
      <ReplyBox
        lastMessageId="msg-1"
        mode="reply"
        onModeChange={() => {}}
        onSent={() => {}}
        originalBody="Original body"
        originalDate="2024-01-01 10:00"
        originalFrom="alice@example.com"
        replyAllRecipients="alice@example.com"
        replyRecipients="alice@example.com"
        subject="Quote"
        threadId="thread-1"
      />
    )
    expect(await screen.findByRole('link', { name: /quote\.pdf/ })).toBeTruthy()
  })

  it('a send names the draft and leaves its uploaded files out of the form', async () => {
    net.upload.mockResolvedValue(NOTES)
    net.saveDraft.mockResolvedValue({ id: 7, success: true })
    openPage(true)
    await waitFor(async () => expect((await editor()).value).toBe(DRAFT.body))
    await attach(new File(['hello'], 'notes.txt', { type: 'text/plain' }))
    await screen.findByRole('link', { name: /notes\.txt/ })
    clickSend()
    await waitFor(() => expect(net.sendMail).toHaveBeenCalledTimes(1))
    const sent = net.sendMail.mock.calls[0][0]
    expect(sent.draftId).toBe(7)
    expect(sent.attachments).toEqual([])
    // the files the server will attach were saved with the draft before the send
    expect(net.saveDraft).toHaveBeenLastCalledWith(
      expect.objectContaining({ attachments: [QUOTE, NOTES], id: 7 })
    )
    // the server deletes the draft it sent
    expect(net.deleteDraft).not.toHaveBeenCalled()
  })

  it('a file whose upload has not finished goes in the form', async () => {
    net.upload.mockReturnValue(new Promise(() => {}))
    net.saveDraft.mockResolvedValue({ id: 7, success: true })
    openPage(true)
    await waitFor(async () => expect((await editor()).value).toBe(DRAFT.body))
    const file = new File(['big'], 'big.mov', { type: 'video/quicktime' })
    await attach(file)
    clickSend()
    await waitFor(() => expect(net.sendMail).toHaveBeenCalledTimes(1))
    const sent = net.sendMail.mock.calls[0][0]
    expect(sent.draftId).toBe(7)
    expect(sent.attachments).toEqual([file])
  })

  it('a send whose draft could not take the files shown does not go', async () => {
    net.saveDraft.mockRejectedValue(new Error('offline'))
    openPage(true)
    await waitFor(async () => expect((await editor()).value).toBe(DRAFT.body))
    fireEvent.click(screen.getByRole('button', { name: 'Remove quote.pdf' }))
    clickSend()
    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(net.sendMail).not.toHaveBeenCalled()
  })

  it("an upload the server refuses says so with the server's reason", async () => {
    net.upload.mockRejectedValue(
      new WireErrorException({
        kind: 'server',
        message: 'the message is larger than the 25 MiB limit',
        status: 413,
      })
    )
    openPage(false)
    await attach(new File(['x'], 'huge.iso', { type: 'application/octet-stream' }))
    const warning = await screen.findByRole('status')
    expect(warning.textContent).toContain('the message is larger than the 25 MiB limit')
    expect(warning.textContent).toContain('huge.iso')
  })
})
