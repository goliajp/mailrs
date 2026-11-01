// The IMAP import from settings: the form starts it and forgets the
// password, progress shows while it runs, and a failure says why in the
// server's words as text.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/imap-import', () => ({
  wireCancelImapImport: vi.fn(),
  wireImapImportStatus: vi.fn(),
  wireStartImapImport: vi.fn(),
}))
vi.mock('@/lib/mail-cache', () => ({ invalidateMail: vi.fn() }))
vi.mock('@/lib/query-client', () => ({ queryClient: { invalidateQueries: vi.fn() } }))

import { ImapImportCard } from '@/components/settings/imap-import-card'
import {
  wireCancelImapImport,
  wireImapImportStatus,
  wireStartImapImport,
} from '@/wire/endpoints/imap-import'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

const REPORT = {
  archived: 0,
  complete: false,
  failed: 0,
  held: 0,
  imported: 4,
  over_quota: false,
  sent: 1,
  skipped_duplicate: 0,
  skipped_junk_or_trash: 0,
  too_large: 0,
}

const BASE = {
  error: null,
  folder: null,
  host: 'imap.gmail.com',
  port: 993,
  report: REPORT,
  username: 'me@gmail.com',
}

function show() {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ImapImportCard />
    </QueryClientProvider>
  )
}

describe('imap import', () => {
  it('starts from the form and clears the password', async () => {
    vi.mocked(wireImapImportStatus).mockResolvedValue({ ...BASE, state: 'none' })
    vi.mocked(wireStartImapImport).mockResolvedValue({ ...BASE, state: 'queued' })
    show()
    fireEvent.change(await screen.findByPlaceholderText('imap.gmail.com'), {
      target: { value: 'imap.gmail.com' },
    })
    fireEvent.change(screen.getByLabelText('User name'), { target: { value: 'me@gmail.com' } })
    const password = screen.getByLabelText('Password or app password')
    fireEvent.change(password, { target: { value: 'app pass' } })
    fireEvent.click(screen.getByText('Start importing'))
    await vi.waitFor(() =>
      expect(wireStartImapImport).toHaveBeenCalledWith({
        host: 'imap.gmail.com',
        password: 'app pass',
        port: 993,
        username: 'me@gmail.com',
      })
    )
    await vi.waitFor(() => expect((password as HTMLInputElement).value).toBe(''))
  })

  it('shows progress while running, and stops on request', async () => {
    vi.mocked(wireImapImportStatus).mockResolvedValue({
      ...BASE,
      folder: 'INBOX',
      state: 'running',
    })
    vi.mocked(wireCancelImapImport).mockResolvedValue()
    show()
    expect(await screen.findByText('Importing from imap.gmail.com: INBOX')).toBeDefined()
    fireEvent.click(screen.getByText('Stop'))
    await vi.waitFor(() => expect(wireCancelImapImport).toHaveBeenCalled())
  })

  it('says why it failed, as text', async () => {
    vi.mocked(wireImapImportStatus).mockResolvedValue({
      ...BASE,
      error: 'the server refused: NO <b>Invalid credentials</b>',
      state: 'failed',
    })
    show()
    expect(
      await screen.findByText(
        'The import stopped: the server refused: NO <b>Invalid credentials</b>'
      )
    ).toBeDefined()
  })

  it('offers to resume an interrupted import', async () => {
    vi.mocked(wireImapImportStatus).mockResolvedValue({ ...BASE, state: 'interrupted' })
    show()
    expect(await screen.findByText(/An import from imap.gmail.com was interrupted/)).toBeDefined()
  })
})
