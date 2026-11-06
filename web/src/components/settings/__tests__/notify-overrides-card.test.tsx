// A muted conversation is listed by its subject, not by its thread id.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/notify', () => ({
  wireNotifyOverrides: vi.fn(),
  wireNotifySender: vi.fn(),
  wireNotifyThread: vi.fn(),
}))
vi.mock('@/lib/query-client', () => ({ queryClient: { invalidateQueries: vi.fn() } }))

import { OverridesCard } from '@/components/settings/notify-cards'
import { wireNotifyOverrides } from '@/wire/endpoints/notify'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

function card() {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <OverridesCard />
    </QueryClientProvider>
  )
}

describe('the per-conversation push choices', () => {
  it('name each conversation by its subject', async () => {
    vi.mocked(wireNotifyOverrides).mockResolvedValue({
      senders: [['news@shop.test', 'never']],
      thread_details: [
        { subject: 'Quarterly report', thread_id: 't1', value: 'mute' },
        { subject: null, thread_id: 't-gone', value: 'follow' },
      ],
      threads: [
        ['t1', 'mute'],
        ['t-gone', 'follow'],
      ],
    })
    card()
    expect(await screen.findByText('Quarterly report')).toBeDefined()
    expect(screen.getByText('A conversation you no longer have')).toBeDefined()
    expect(screen.getByText('news@shop.test')).toBeDefined()
    expect(screen.queryByText('t1')).toBeNull()
  })

  it('fall back to the id when the server sends no subjects', async () => {
    vi.mocked(wireNotifyOverrides).mockResolvedValue({
      senders: [],
      thread_details: [],
      threads: [['t1', 'mute']],
    })
    card()
    expect(await screen.findByText('t1')).toBeDefined()
  })
})
