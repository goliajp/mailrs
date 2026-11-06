// A message's menu blocks its sender: the item names the address, asks
// first (saying so when the address is on the allowed list), blocks and
// moves the conversation in one call, and offers an undo that puts back
// what blocking took. An address already blocked is offered Unblock; the
// reader's own message is offered neither.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

const toasts = vi.hoisted(() => ({
  calls: [] as { action?: { label: string; onClick: () => void }; msg: string }[],
}))

vi.mock('@goliapkg/gds', async () => {
  const real = await vi.importActual<Record<string, unknown>>('@goliapkg/gds')
  return {
    ...real,
    toast: {
      error: vi.fn(),
      success: (msg: string, opts?: { action?: { label: string; onClick: () => void } }) => {
        toasts.calls.push({ action: opts?.action, msg })
      },
    },
  }
})
vi.mock('@/wire/endpoints/block-sender', () => ({
  wireBlockSender: vi.fn(),
  wireUnblockSender: vi.fn(),
}))
vi.mock('@/wire/endpoints/settings', () => ({
  wireListSenders: vi.fn(),
  wireRemoveSender: vi.fn(),
}))
vi.mock('@/lib/api', () => ({ recordFeedback: vi.fn() }))
vi.mock('@/lib/mail-cache', () => ({ invalidateMail: vi.fn() }))
vi.mock('@/lib/query-client', () => ({ queryClient: { invalidateQueries: vi.fn() } }))

import { MessageMenu } from '@/components/message-menu'
import { invalidateMail } from '@/lib/mail-cache'
import { queryClient } from '@/lib/query-client'
import { wireBlockSender, wireUnblockSender } from '@/wire/endpoints/block-sender'
import { wireListSenders, wireRemoveSender } from '@/wire/endpoints/settings'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  toasts.calls.length = 0
})

const SENDER = 'Friend@Partner.example'
const ADDRESS = 'friend@partner.example'

function lists(blocked: string[], allowed: string[]) {
  vi.mocked(wireListSenders).mockImplementation((list) => {
    if (list === 'blacklist') return Promise.resolve(blocked)
    return Promise.resolve(allowed)
  })
}

function openMenu(own = false) {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <MessageMenu own={own} senderEmail={SENDER} threadId="t-1" />
    </QueryClientProvider>
  )
  fireEvent.click(screen.getByTitle('Sender feedback'))
}

describe('blocking a sender from the message menu', () => {
  it('asks, blocks, moves the conversation, and the undo puts it all back', async () => {
    lists([], [ADDRESS])
    vi.mocked(wireBlockSender).mockResolvedValue({ address: ADDRESS, was_allowed: true })
    vi.mocked(wireUnblockSender).mockResolvedValue()
    openMenu()

    fireEvent.click(await screen.findByText(`Block ${ADDRESS}`))
    expect(wireBlockSender).not.toHaveBeenCalled()
    expect(screen.getByText(/Mail from this address will go to Junk/)).toBeDefined()
    expect(screen.getByText(/Settings → Senders/)).toBeDefined()
    // on the allowed list: blocking takes it off, and says so first
    expect(await screen.findByText(/is on your Always allowed list/)).toBeDefined()

    fireEvent.click(screen.getByText('Confirm'))
    await vi.waitFor(() => expect(wireBlockSender).toHaveBeenCalledWith('t-1', ADDRESS))
    await vi.waitFor(() => expect(toasts.calls).toHaveLength(1))
    expect(toasts.calls[0].msg).toBe(`Blocked ${ADDRESS}`)
    expect(invalidateMail).toHaveBeenCalled()
    expect(queryClient.invalidateQueries).toHaveBeenCalledWith({
      queryKey: ['settings', 'senders'],
    })

    const undo = toasts.calls[0].action
    expect(undo?.label).toBe('Undo')
    undo?.onClick()
    // back on the allowed list it came off
    await vi.waitFor(() => expect(wireUnblockSender).toHaveBeenCalledWith('t-1', ADDRESS, true))
    await vi.waitFor(() => expect(toasts.calls).toHaveLength(2))
    expect(toasts.calls[1].msg).toContain(`Unblocked ${ADDRESS}`)
  })

  it('cancelling blocks nothing', async () => {
    lists([], [])
    openMenu()
    fireEvent.click(await screen.findByText(`Block ${ADDRESS}`))
    expect(screen.queryByText(/is on your Always allowed list/)).toBeNull()
    fireEvent.click(screen.getByText('Cancel'))
    expect(wireBlockSender).not.toHaveBeenCalled()
    expect(screen.getByText(`Block ${ADDRESS}`)).toBeDefined()
  })

  it('an address already blocked is offered Unblock, which only takes it off the list', async () => {
    lists([ADDRESS], [])
    vi.mocked(wireRemoveSender).mockResolvedValue()
    openMenu()
    fireEvent.click(await screen.findByText(`Unblock ${ADDRESS}`))
    expect(screen.queryByText(`Block ${ADDRESS}`)).toBeNull()
    await vi.waitFor(() => expect(wireRemoveSender).toHaveBeenCalledWith('blacklist', ADDRESS))
    expect(wireBlockSender).not.toHaveBeenCalled()
    expect(wireUnblockSender).not.toHaveBeenCalled()
  })

  it("the reader's own message offers neither", async () => {
    lists([], [])
    openMenu(true)
    expect(await screen.findByText('Mark important')).toBeDefined()
    expect(screen.queryByText(/Block /)).toBeNull()
    expect(wireListSenders).not.toHaveBeenCalled()
  })
})
