// The sectioned inbox's bar: shown only for the Inbox with the setting on,
// switches the section, and accepts the senders of New.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/senders', () => ({ wireAcceptSenders: vi.fn() }))
vi.mock('@/wire/endpoints/mail-settings', () => ({
  wireGetSectionedInbox: vi.fn(),
  wireSetSectionedInbox: vi.fn(),
}))
vi.mock('@/lib/mail-cache', () => ({ invalidateMail: vi.fn() }))
vi.mock('@/hooks/use-current-list', () => ({
  useConversationRows: () => ({ rows: [{ participants: ['Eve <eve@far.test>'] }] }),
}))
vi.mock('@/hooks/use-open-list', () => ({ useOpenList: () => openList }))
vi.mock('@/hooks/use-mail-queries', () => ({ useCategoriesQuery: () => ({ data: [] }) }))

const openList = vi.fn()

import { FilterBar } from '@/components/conversation-list-filter-bar'
import { InboxSectionBar } from '@/components/inbox-section-bar'
import { invalidateMail } from '@/lib/mail-cache'
import { sectionedInboxKey } from '@/lib/sectioned-inbox'
import { inboxSectionAtom } from '@/store/inbox-sections'
import { activeListAtom } from '@/store/ui'
import { wireGetSectionedInbox } from '@/wire/endpoints/mail-settings'
import { wireAcceptSenders } from '@/wire/endpoints/senders'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})

function show(
  sectioned: boolean,
  list: 'inbox' | 'np' | 'unread' = 'inbox',
  bar: () => React.ReactNode = () => <InboxSectionBar />
) {
  const store = createStore()
  store.set(activeListAtom, list)
  vi.mocked(wireGetSectionedInbox).mockResolvedValue({ enabled: sectioned })
  const client = new QueryClient()
  client.setQueryData(sectionedInboxKey, { enabled: sectioned })
  render(
    <QueryClientProvider client={client}>
      <Provider store={store}>{bar()}</Provider>
    </QueryClientProvider>
  )
  return store
}

describe('InboxSectionBar', () => {
  it('is not there unless the Inbox is shown by section', () => {
    show(false)
    expect(screen.queryByRole('tablist')).toBeNull()
    cleanup()
    show(true, 'unread')
    expect(screen.queryByRole('tablist')).toBeNull()
  })

  it('switches sections, and opens notifications in their own tab', () => {
    const store = show(true)
    fireEvent.click(screen.getByRole('tab', { name: 'Follow up' }))
    expect(store.get(inboxSectionAtom)).toBe('followup')
    fireEvent.click(screen.getByText(/Notifications and promotions/))
    expect(openList).toHaveBeenCalledWith('np')
  })

  it('accepts the senders shown in New', async () => {
    vi.mocked(wireAcceptSenders).mockResolvedValue({ accepted: 1 })
    const store = show(true)
    expect(screen.queryByText('Accept all')).toBeNull()
    store.set(inboxSectionAtom, 'new')
    fireEvent.click(await screen.findByText('Accept all'))
    await vi.waitFor(() => expect(wireAcceptSenders).toHaveBeenCalledWith(['eve@far.test']))
    await vi.waitFor(() => expect(invalidateMail).toHaveBeenCalled())
  })

  it('in Notifications and promotions, shows it as chosen and leads back to each section', () => {
    const store = show(true, 'np')
    const feed = screen.getByRole('tab', { name: 'Notifications and promotions' })
    expect(feed.getAttribute('aria-selected')).toBe('true')
    expect(feed.tabIndex).toBe(0)
    const sections = screen.getAllByRole('tab').filter((tab) => tab !== feed)
    expect(sections).toHaveLength(4)
    for (const tab of sections) {
      expect(tab.textContent?.startsWith('← ')).toBe(true)
      expect(tab.getAttribute('aria-selected')).toBe('false')
      expect(tab.tabIndex).toBe(-1)
    }
    fireEvent.click(screen.getByRole('tab', { name: '← Follow up' }))
    expect(store.get(inboxSectionAtom)).toBe('followup')
    expect(openList).toHaveBeenCalledWith('inbox')
  })

  it('in the Inbox, marks the way to Notifications and promotions with an arrow', () => {
    show(true)
    const feed = screen.getByRole('tab', { name: 'Notifications and promotions →' })
    expect(feed.getAttribute('aria-selected')).toBe('false')
    expect(screen.getByRole('tab', { name: 'Follow up' }).textContent).toBe('Follow up')
  })

  it.each(['inbox', 'np'] as const)(
    'moves between Other and the feed chip by arrow keys in %s',
    (list) => {
      show(true, list)
      const tabs = screen.getAllByRole('tab')
      expect(tabs).toHaveLength(5)
      const [other, feed] = tabs.slice(3)
      other.focus()
      fireEvent.keyDown(other, { key: 'ArrowRight' })
      expect(document.activeElement).toBe(feed)
      fireEvent.keyDown(feed, { key: 'ArrowLeft' })
      expect(document.activeElement).toBe(other)
    }
  )

  it('sits in the filter row from xl and on its own row below it', () => {
    show(true, 'inbox', () => (
      <>
        <FilterBar />
        <InboxSectionBar />
      </>
    ))
    const bars = screen.getAllByRole('tablist', { name: 'Inbox sections' })
    expect(bars).toHaveLength(2)
    const filterButton = screen.getByRole('button', { name: 'Show or hide filters' })
    const filterRow = filterButton.parentElement?.parentElement as HTMLElement
    const [inRow, own] = bars
    const inRowBox = inRow.parentElement as HTMLElement
    expect(inRowBox.parentElement).toBe(filterRow)
    expect(inRowBox.className.split(' ')).toEqual(expect.arrayContaining(['hidden', 'xl:flex']))
    const ownBox = own.parentElement as HTMLElement
    expect(filterRow.contains(ownBox)).toBe(false)
    expect(ownBox.className.split(' ')).toContain('xl:hidden')
  })

  it('keeps accept-all for New in the filter row as well', async () => {
    const store = show(true, 'inbox', () => <FilterBar />)
    store.set(inboxSectionAtom, 'new')
    expect(await screen.findByText('Accept all')).toBeTruthy()
  })
})
