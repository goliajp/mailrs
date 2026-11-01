// Masked addresses in settings: listed with their site and last message,
// made from a site and a note, turned off and on, and deleted only after
// the reader confirms.

import { QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/masked-email', () => ({
  wireCreateMaskedEmail: vi.fn(),
  wireDeleteMaskedEmail: vi.fn(),
  wireListMaskedEmails: vi.fn(),
  wireSetMaskedEmailState: vi.fn(),
}))

import { MaskedEmailsSection } from '@/components/settings/masked-emails-section'
import { queryClient } from '@/lib/query-client'
import {
  wireCreateMaskedEmail,
  wireDeleteMaskedEmail,
  wireListMaskedEmails,
  wireSetMaskedEmailState,
} from '@/wire/endpoints/masked-email'

afterEach(() => {
  cleanup()
  queryClient.clear()
  vi.clearAllMocks()
})

const MASK = {
  createdAt: 1_700_000_000,
  createdBy: 'web',
  description: 'orders',
  email: 'amber.otter.0042@golia.jp',
  forDomain: 'shop.example',
  id: 'me-amber.otter.0042',
  lastMessageAt: null,
  owner: 'me@golia.jp',
  state: 'enabled' as const,
  url: null,
}

function show() {
  render(
    <QueryClientProvider client={queryClient}>
      <MaskedEmailsSection />
    </QueryClientProvider>
  )
}

describe('masked addresses', () => {
  it('lists each mask with its site, and makes a new one', async () => {
    vi.mocked(wireListMaskedEmails).mockResolvedValue([MASK])
    vi.mocked(wireCreateMaskedEmail).mockResolvedValue({
      ...MASK,
      email: 'brave.heron.0001@golia.jp',
    })
    show()
    expect(await screen.findByText(MASK.email)).toBeDefined()
    expect(screen.getByText('shop.example · orders')).toBeDefined()
    expect(screen.getByText('No mail yet')).toBeDefined()
    fireEvent.change(screen.getByLabelText('Site'), { target: { value: ' news.example ' } })
    fireEvent.change(screen.getByLabelText('Note'), { target: { value: 'letters' } })
    fireEvent.click(screen.getByText('New masked address'))
    await vi.waitFor(() =>
      expect(wireCreateMaskedEmail).toHaveBeenCalledWith({
        description: 'letters',
        forDomain: 'news.example',
      })
    )
  })

  it('turns a mask off and on', async () => {
    vi.mocked(wireListMaskedEmails).mockResolvedValue([
      MASK,
      { ...MASK, email: 'x.y.0002@golia.jp', state: 'disabled' },
    ])
    vi.mocked(wireSetMaskedEmailState).mockResolvedValue(MASK)
    show()
    fireEvent.click(await screen.findByText('Turn off'))
    await vi.waitFor(() =>
      expect(wireSetMaskedEmailState).toHaveBeenCalledWith(MASK.email, 'disabled')
    )
    fireEvent.click(screen.getByText('Turn on'))
    await vi.waitFor(() =>
      expect(wireSetMaskedEmailState).toHaveBeenCalledWith('x.y.0002@golia.jp', 'enabled')
    )
  })

  it('deletes only after the reader confirms', async () => {
    vi.mocked(wireListMaskedEmails).mockResolvedValue([MASK])
    vi.mocked(wireDeleteMaskedEmail).mockResolvedValue()
    show()
    fireEvent.click(await screen.findByText('Delete'))
    expect(wireDeleteMaskedEmail).not.toHaveBeenCalled()
    expect(screen.getByText(/will never be given out again/)).toBeDefined()
    const confirm = screen.getAllByRole('button').find((b) => b.textContent === 'Confirm')
    expect(confirm).toBeDefined()
    fireEvent.click(confirm!)
    await vi.waitFor(() => expect(wireDeleteMaskedEmail).toHaveBeenCalledWith(MASK.email))
  })
})
