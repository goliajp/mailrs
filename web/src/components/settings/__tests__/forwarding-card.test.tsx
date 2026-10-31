// Forwarding in settings: an address waiting for its code asks for it, and
// what is on can be stopped.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/mail-settings', () => ({
  wireGetForwarding: vi.fn(),
  wireRequestForwarding: vi.fn(),
  wireStopForwarding: vi.fn(),
  wireVerifyForwarding: vi.fn(),
}))
vi.mock('@/lib/query-client', () => ({
  queryClient: { invalidateQueries: vi.fn() },
}))

import { ForwardingCard } from '@/components/settings/forwarding-card'
import {
  wireGetForwarding,
  wireRequestForwarding,
  wireStopForwarding,
  wireVerifyForwarding,
} from '@/wire/endpoints/mail-settings'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

function show() {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ForwardingCard />
    </QueryClientProvider>
  )
}

describe('forwarding', () => {
  it('asks for the code sent to a pending address', async () => {
    vi.mocked(wireGetForwarding).mockResolvedValue({
      address: '',
      keep_copy: true,
      pending_address: 'me@elsewhere.example',
    })
    vi.mocked(wireVerifyForwarding).mockResolvedValue({ address: 'me@elsewhere.example' })
    show()
    fireEvent.change(await screen.findByLabelText('Code'), { target: { value: ' 123456 ' } })
    fireEvent.click(screen.getByText('Confirm'))
    await vi.waitFor(() => expect(wireVerifyForwarding).toHaveBeenCalledWith('123456'))
  })

  it('sends a code to a new address and can stop what is on', async () => {
    vi.mocked(wireGetForwarding).mockResolvedValue({
      address: 'old@elsewhere.example',
      keep_copy: false,
      pending_address: '',
    })
    vi.mocked(wireRequestForwarding).mockResolvedValue()
    vi.mocked(wireStopForwarding).mockResolvedValue()
    show()
    expect(await screen.findByText(/without keeping a copy here/)).toBeDefined()
    fireEvent.change(screen.getByPlaceholderText('me@elsewhere.example'), {
      target: { value: 'new@elsewhere.example' },
    })
    fireEvent.click(screen.getByLabelText('Keep a copy here'))
    fireEvent.click(screen.getByText('Send code'))
    await vi.waitFor(() =>
      expect(wireRequestForwarding).toHaveBeenCalledWith('new@elsewhere.example', false)
    )
    fireEvent.click(screen.getByText('Stop'))
    await vi.waitFor(() => expect(wireStopForwarding).toHaveBeenCalled())
  })
})
