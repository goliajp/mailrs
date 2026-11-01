// Push settings: the rule form's body, the quiet-hours clock, the week's
// counters, and a server without Web Push saying so instead of showing zeros.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/web-push', () => ({
  wireWebPushState: vi.fn(),
  wireWebPushSubscribe: vi.fn(),
  wireWebPushUnsubscribe: vi.fn(),
}))
vi.mock('@/wire/endpoints/sender-rules', () => ({
  wireDeleteSenderRule: vi.fn(),
  wireListSenderRules: vi.fn(),
  wirePutSenderRule: vi.fn(),
}))
vi.mock('@/lib/query-client', () => ({ queryClient: { invalidateQueries: vi.fn() } }))
vi.mock('@/lib/mail-cache', () => ({ invalidateMail: vi.fn() }))

import { WebPushCard } from '@/components/settings/notify-cards'
import {
  clock,
  draftBody,
  EMPTY_DRAFT,
  minutesOf,
  weekTotals,
} from '@/components/settings/notify-shared'
import { SenderRulesCard } from '@/components/settings/sender-rules-card'
import { wireListSenderRules, wirePutSenderRule } from '@/wire/endpoints/sender-rules'
import { wireWebPushState } from '@/wire/endpoints/web-push'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

function wrap(node: React.ReactNode) {
  return render(<QueryClientProvider client={new QueryClient()}>{node}</QueryClientProvider>)
}

describe('push settings', () => {
  it('a rule form says nothing until it names a bucket or a limit', () => {
    expect(draftBody({ ...EMPTY_DRAFT, address: 'a@x' })).toBeNull()
    expect(draftBody({ ...EMPTY_DRAFT, keep: '0' })).toBeNull()
    expect(draftBody({ ...EMPTY_DRAFT, keep: '2.5' })).toBeNull()
    expect(draftBody({ ...EMPTY_DRAFT, days: '30', disposal: 'trash' })).toEqual({
      bucket: undefined,
      disposal: 'trash',
      keep_latest: undefined,
      older_than_days: 30,
    })
  })

  it('quiet hours round-trip through the clock', () => {
    expect(clock(22 * 60 + 5)).toBe('22:05')
    expect(minutesOf('07:30')).toBe(450)
  })

  it('sums a channel over the week', () => {
    const days: { counts: Record<string, Record<string, number>>; day: string }[] = [
      { counts: { web: { clicked: 1, sent: 2 } }, day: '20261101' },
      { counts: { apns: { sent: 9 }, web: { sent: 3 } }, day: '20261031' },
    ]
    expect(weekTotals(days, 'web')).toEqual({ clicked: 1, sent: 5 })
    expect(weekTotals(days, 'fcm')).toEqual({})
  })

  it('without a VAPID key the card says Web Push is not enabled', async () => {
    vi.mocked(wireWebPushState).mockResolvedValue({ enabled: false })
    wrap(<WebPushCard />)
    expect(await screen.findByText('Web Push is not enabled on this server')).toBeDefined()
    expect(screen.queryByText('Turn on in this browser')).toBeNull()
  })

  it('saves a sender rule and says how much it let go of', async () => {
    vi.mocked(wireListSenderRules).mockResolvedValue([])
    vi.mocked(wirePutSenderRule).mockResolvedValue({ disposed: 4 })
    wrap(<SenderRulesCard />)
    fireEvent.change(screen.getByLabelText('Sender address'), {
      target: { value: 'news@shop.test' },
    })
    fireEvent.change(screen.getByLabelText('Keep only the newest (conversations)'), {
      target: { value: '3' },
    })
    fireEvent.click(screen.getByText('Save rule'))
    await vi.waitFor(() =>
      expect(wirePutSenderRule).toHaveBeenCalledWith('news@shop.test', {
        bucket: undefined,
        disposal: 'archive',
        keep_latest: 3,
        older_than_days: undefined,
      })
    )
  })
})
