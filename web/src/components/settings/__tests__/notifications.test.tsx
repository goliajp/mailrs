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
    vi.mocked(wireWebPushState).mockResolvedValue({ enabled: false, removed: [] })
    wrap(<WebPushCard />)
    expect(await screen.findByText('Web Push is not enabled on this server')).toBeDefined()
    expect(screen.queryByText('Turn on in this browser')).toBeNull()
  })

  describe('a browser removed from another device', () => {
    const endpoint = 'https://fcm.googleapis.com/fcm/send/c1d2e3'
    // sha256 of the endpoint, as the server files it (shasum -a 256)
    const id = 'cbbdb8ce49e30d56eaebe6cfd0ee35a847f725e08bfb98abb36aff0c324f1b17'

    function subscribed() {
      const getSubscription = () => Promise.resolve({ endpoint })
      vi.stubGlobal('PushManager', class {})
      vi.stubGlobal('Notification', class {})
      Object.defineProperty(navigator, 'serviceWorker', {
        configurable: true,
        value: { getRegistration: () => Promise.resolve({ pushManager: { getSubscription } }) },
      })
    }

    afterEach(() => {
      vi.unstubAllGlobals()
      Reflect.deleteProperty(navigator, 'serviceWorker')
    })

    it('shows as off, says why, and offers to turn on again', async () => {
      subscribed()
      vi.mocked(wireWebPushState).mockResolvedValue({
        enabled: true,
        public_key: 'k',
        removed: [id],
      })
      wrap(<WebPushCard />)
      expect(
        await screen.findByText('Removed from another device; turn on again to receive pushes')
      ).toBeDefined()
      expect(screen.getByText('Turn on in this browser')).toBeDefined()
      expect(screen.queryByText('On in this browser')).toBeNull()
    })

    it('one still listed is on, without the line', async () => {
      subscribed()
      vi.mocked(wireWebPushState).mockResolvedValue({ enabled: true, public_key: 'k', removed: [] })
      wrap(<WebPushCard />)
      expect(await screen.findByText('On in this browser')).toBeDefined()
      expect(screen.queryByTestId('web-push-removed')).toBeNull()
    })
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
