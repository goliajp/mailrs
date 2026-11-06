// The swipe setting shows what the server has and saves a change there, so
// the phone apps read the same value.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/mail-settings', () => ({
  wireGetSwipe: vi.fn(),
  wireSetSwipe: vi.fn(),
}))

import { SwipeCard } from '@/components/settings/swipe-card'
import { wireGetSwipe, wireSetSwipe } from '@/wire/endpoints/mail-settings'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('swipe setting', () => {
  it('offers the four choices per side and saves the one picked', async () => {
    vi.mocked(wireGetSwipe).mockResolvedValue({ left: 'trash', right: 'archive' })
    vi.mocked(wireSetSwipe).mockResolvedValue({ left: 'trash', right: 'read' })
    render(
      <QueryClientProvider client={new QueryClient()}>
        <SwipeCard />
      </QueryClientProvider>
    )
    const right = (await screen.findByLabelText('Swipe right')) as HTMLSelectElement
    await vi.waitFor(() => expect(right.value).toBe('archive'))
    expect((screen.getByLabelText('Swipe left') as HTMLSelectElement).value).toBe('trash')
    expect(Array.from(right.options).map((o) => o.value)).toEqual([
      'archive',
      'trash',
      'read',
      'none',
    ])

    fireEvent.change(right, { target: { value: 'read' } })
    await vi.waitFor(() =>
      expect(vi.mocked(wireSetSwipe).mock.calls[0]?.[0]).toEqual({ left: 'trash', right: 'read' })
    )
  })
})
