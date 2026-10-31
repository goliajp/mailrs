// Sessions in settings: this device cannot be signed out from its own row,
// the others can, one at a time or all together.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/sessions', () => ({
  wireEndOtherSessions: vi.fn(),
  wireEndSession: vi.fn(),
  wireListSessions: vi.fn(),
}))
vi.mock('@/lib/query-client', () => ({ queryClient: { invalidateQueries: vi.fn() } }))

import { SessionsCard } from '@/components/settings/sessions-card'
import { wireEndOtherSessions, wireEndSession, wireListSessions } from '@/wire/endpoints/sessions'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

const HERE = {
  created_at: 1,
  current: true,
  id: 'aaaaaaaaaaaaaaaa',
  ip: '203.0.113.9',
  last_seen: 2,
  user_agent: 'Mozilla/5.0 (Macintosh) Version/18.1 Safari/605.1.15',
}
const THERE = {
  ...HERE,
  current: false,
  id: 'bbbbbbbbbbbbbbbb',
  user_agent: 'Mozilla/5.0 (X11; Linux x86_64; rv:131.0) Gecko/20100101 Firefox/131.0',
}

describe('sessions', () => {
  it('sign out another one, or all the others', async () => {
    vi.mocked(wireListSessions).mockResolvedValue([HERE, THERE])
    vi.mocked(wireEndSession).mockResolvedValue()
    vi.mocked(wireEndOtherSessions).mockResolvedValue({ ended: 1 })
    render(
      <QueryClientProvider client={new QueryClient()}>
        <SessionsCard />
      </QueryClientProvider>
    )
    expect(await screen.findByText('This device')).toBeDefined()
    expect(screen.getByText('Firefox on Linux')).toBeDefined()
    // one row has the button: the other session's
    const buttons = screen.getAllByText('Sign out')
    expect(buttons).toHaveLength(1)
    fireEvent.click(buttons[0])
    await vi.waitFor(() => expect(wireEndSession).toHaveBeenCalledWith('bbbbbbbbbbbbbbbb'))
    fireEvent.click(screen.getByText('Sign out of all other sessions'))
    await vi.waitFor(() => expect(wireEndOtherSessions).toHaveBeenCalled())
  })
})
