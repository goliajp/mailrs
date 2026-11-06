// A page inside the pane is rendered once, at either width: two copies
// shared one radio group (the visible mode showed nothing checked) and
// sent every query twice.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/notify', () => ({
  wireImportantContacts: vi.fn(() => Promise.resolve([])),
  wireNotifyOverrides: vi.fn(() => Promise.resolve({ senders: [], threads: [] })),
  wireNotifyPrefs: vi.fn(() =>
    Promise.resolve({ hide_content: false, mode: 'important', utc_offset_minutes: 0 })
  ),
  wireNotifySender: vi.fn(),
  wireNotifyStats: vi.fn(() => Promise.resolve([])),
  wireNotifyThread: vi.fn(),
  wirePutNotifyPrefs: vi.fn(),
  wireSetImportant: vi.fn(),
}))
vi.mock('@/wire/endpoints/web-push', () => ({
  wireWebPushState: vi.fn(() => Promise.resolve({ enabled: false, removed: [] })),
}))

import { NotificationsSection } from '@/components/settings/notifications-section'
import { PagePane } from '@/layouts/page-pane'
import { wireNotifyPrefs } from '@/wire/endpoints/notify'

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.clearAllMocks()
})

function setWidth(mobile: boolean) {
  vi.spyOn(window, 'matchMedia').mockImplementation(
    (query: string) =>
      ({
        matches: mobile,
        media: query,
        addEventListener: () => {},
        removeEventListener: () => {},
      }) as unknown as MediaQueryList
  )
}

describe('PagePane', () => {
  for (const [layout, mobile] of [
    ['desktop', false],
    ['phone', true],
  ] as const) {
    it(`renders the page once (${layout})`, async () => {
      setWidth(mobile)
      render(
        <QueryClientProvider client={new QueryClient()}>
          <PagePane>
            <NotificationsSection />
          </PagePane>
        </QueryClientProvider>
      )
      await screen.findAllByText('Important contacts only')
      const radios = document.querySelectorAll<HTMLInputElement>('input[name="notify-mode"]')
      expect(radios).toHaveLength(4)
      const checked = [...radios].filter((r) => r.checked)
      expect(checked).toHaveLength(1)
      expect(checked[0].closest('label')?.textContent).toBe('Important contacts only')
      expect(wireNotifyPrefs).toHaveBeenCalledTimes(1)
    })
  }
})
