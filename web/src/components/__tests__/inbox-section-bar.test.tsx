// The sectioned inbox's bar: shown for the Inbox and Notifications with the
// setting on. The whole inbox until a chip is chosen; Follow up and New only
// while they hold something, with how many; accepts the senders of New.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
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
vi.mock('@/hooks/use-mail-queries', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/hooks/use-mail-queries')>()),
  useCategoriesQuery: () => ({ data: [] }),
}))
vi.mock('@/wire/client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/wire/client')>()),
  wireFetch: vi.fn(),
}))

const openList = vi.fn()

import { FilterBar } from '@/components/conversation-list-filter-bar'
import { InboxSectionBar } from '@/components/inbox-section-bar'
import { invalidateMail } from '@/lib/mail-cache'
import { mailFilterInputsIn, mailFiltersOf } from '@/lib/mail-filters'
import { sectionedInboxKey } from '@/lib/sectioned-inbox'
import { inboxSectionAtom } from '@/store/inbox-sections'
import { activeListAtom } from '@/store/ui'
import { wireFetch } from '@/wire/client'
import { wireGetSectionedInbox } from '@/wire/endpoints/mail-settings'
import { wireAcceptSenders } from '@/wire/endpoints/senders'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})

/** How many conversations each section's list answers with, by its `section=` value. */
type Held = { awaiting?: number; new?: number; reply?: number }

function answer(held: Held) {
  const paths: string[] = []
  vi.mocked(wireFetch).mockImplementation((async (_schema: unknown, req: { path: string }) => {
    paths.push(req.path)
    const section = /[?&]section=([a-z]+)/.exec(req.path)?.[1] as keyof Held | undefined
    const n = (section && held[section]) || 0
    const items = Array.from({ length: n }, (_, i) => ({ thread_id: `${section}${i}` }))
    return { items }
  }) as never)
  return paths
}

function show(
  sectioned: boolean,
  list: 'inbox' | 'np' | 'unread' = 'inbox',
  held: Held = {},
  bar: () => React.ReactNode = () => <InboxSectionBar />
) {
  const paths = answer(held)
  const store = createStore()
  store.set(activeListAtom, list)
  vi.mocked(wireGetSectionedInbox).mockResolvedValue({ enabled: sectioned })
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  client.setQueryData(sectionedInboxKey, { enabled: sectioned })
  render(
    <QueryClientProvider client={client}>
      <Provider store={store}>{bar()}</Provider>
    </QueryClientProvider>
  )
  return { paths, store }
}

const names = () => screen.getAllByRole('tab').map((t) => t.textContent)

describe('InboxSectionBar', () => {
  it('is not there unless the Inbox or Notifications is shown by section', () => {
    show(false)
    expect(screen.queryByRole('tablist')).toBeNull()
    cleanup()
    show(true, 'unread')
    expect(screen.queryByRole('tablist')).toBeNull()
  })

  it('opens on the whole inbox: no chip chosen and no section asked for', async () => {
    const { store } = show(true, 'inbox', { reply: 1 })
    await screen.findByRole('tab', { name: 'To reply 1' })
    expect(store.get(inboxSectionAtom)).toBeNull()
    const filters = mailFiltersOf({ ...mailFilterInputsIn(store), list: 'inbox', sectioned: true })
    expect(filters?.section).toBeUndefined()
    for (const tab of screen.getAllByRole('tab')) {
      expect(tab.getAttribute('aria-selected')).toBe('false')
    }
    expect(screen.getAllByRole('tab')[0].tabIndex).toBe(0)
  })

  it('leaves Other out, and every section out while it is empty', async () => {
    const { paths } = show(true, 'inbox', {}, () => <FilterBar />)
    await vi.waitFor(() => expect(paths.filter((p) => p.includes('section=')).length).toBe(3))
    expect(paths.some((p) => p.includes('section=new'))).toBe(true)
    expect(paths.some((p) => p.includes('section=awaiting'))).toBe(true)
    expect(paths.some((p) => p.includes('section=reply'))).toBe(true)
    // in the filter row the way on to Notifications is all that is left,
    // and it is where Tab lands
    const inRow = within(screen.getByRole('tablist', { name: 'Inbox sections' }))
    const tabs = inRow.getAllByRole('tab')
    expect(tabs.map((t) => t.textContent)).toEqual(['Notifications and promotions →'])
    expect(tabs[0].tabIndex).toBe(0)
  })

  it('below xl, shows no row of its own when every section is empty', async () => {
    const { paths } = show(true)
    await vi.waitFor(() => expect(paths.filter((p) => p.includes('section=')).length).toBe(3))
    // the tabs right above already lead to Notifications
    expect(screen.queryByRole('tablist')).toBeNull()
  })

  it('shows each section with how many it holds, from its own list', async () => {
    show(true, 'inbox', { awaiting: 1, new: 2, reply: 3 })
    expect(await screen.findByRole('tab', { name: 'New senders 2' })).toBeTruthy()
    await screen.findByRole('tab', { name: 'To reply 3' })
    expect(names()).toEqual([
      'To reply 3',
      'Follow up 1',
      'New senders 2',
      'Notifications and promotions →',
    ])
    expect(screen.getByRole('tab', { name: 'To reply 3' }).tabIndex).toBe(0)
  })

  it('in Notifications, shows no row while there is no section to go back to', async () => {
    const { paths } = show(true, 'np')
    await vi.waitFor(() => expect(paths.some((p) => p.includes('section=reply'))).toBe(true))
    await vi.waitFor(() => expect(paths.filter((p) => p.includes('section=')).length).toBe(3))
    expect(screen.queryByRole('tablist')).toBeNull()
  })

  it('keeps To reply while it is chosen and empty', async () => {
    const { store } = show(true, 'inbox', { reply: 0 })
    store.set(inboxSectionAtom, 'reply')
    const chosen = await screen.findByRole('tab', { name: 'To reply' })
    expect(chosen.getAttribute('aria-selected')).toBe('true')
    fireEvent.click(chosen)
    expect(screen.queryByRole('tab', { name: /To reply/ })).toBeNull()
  })

  it('says 50+ when a whole page came back', async () => {
    show(true, 'inbox', { new: 50 })
    expect(await screen.findByRole('tab', { name: 'New senders 50+' })).toBeTruthy()
  })

  it('keeps a chosen section that empties until the reader leaves it', async () => {
    const { store } = show(true, 'inbox', { new: 0 })
    store.set(inboxSectionAtom, 'new')
    const chosen = await screen.findByRole('tab', { name: 'New senders' })
    expect(chosen.getAttribute('aria-selected')).toBe('true')
    fireEvent.click(chosen)
    expect(store.get(inboxSectionAtom)).toBeNull()
    expect(screen.queryByRole('tab', { name: /New senders/ })).toBeNull()
  })

  it('switches sections, clears the chosen one on a second click, and opens notifications', async () => {
    const { store } = show(true, 'inbox', { awaiting: 3 })
    const followup = await screen.findByRole('tab', { name: 'Follow up 3' })
    fireEvent.click(followup)
    expect(store.get(inboxSectionAtom)).toBe('followup')
    expect(followup.tabIndex).toBe(0)
    fireEvent.click(followup)
    expect(store.get(inboxSectionAtom)).toBeNull()
    fireEvent.click(screen.getByRole('tab', { name: 'Notifications and promotions →' }))
    expect(openList).toHaveBeenCalledWith('np')
  })

  it('in Notifications, shows it as chosen and leads back by the same chips', async () => {
    const { store } = show(true, 'np', { new: 4 })
    const back = await screen.findByRole('tab', { name: '← New senders 4' })
    expect(names()).toEqual(['← New senders 4', 'Notifications and promotions'])
    const feed = screen.getByRole('tab', { name: 'Notifications and promotions' })
    expect(feed.getAttribute('aria-selected')).toBe('true')
    expect(feed.tabIndex).toBe(0)
    fireEvent.click(back)
    expect(store.get(inboxSectionAtom)).toBe('new')
    expect(openList).toHaveBeenCalledWith('inbox')
  })

  it.each(['inbox', 'np'] as const)(
    'moves across the shown chips by arrow keys in %s',
    async (list) => {
      show(true, list, { awaiting: 1, reply: 1 })
      await screen.findByRole('tab', { name: /Follow up 1/ })
      const tabs = screen.getAllByRole('tab')
      expect(tabs).toHaveLength(3)
      const [first, followup, feed] = tabs
      followup.focus()
      fireEvent.keyDown(followup, { key: 'ArrowRight' })
      expect(document.activeElement).toBe(feed)
      fireEvent.keyDown(feed, { key: 'ArrowRight' })
      expect(document.activeElement).toBe(first)
      fireEvent.keyDown(first, { key: 'ArrowLeft' })
      expect(document.activeElement).toBe(feed)
    }
  )

  it('accepts the senders shown in New', async () => {
    vi.mocked(wireAcceptSenders).mockResolvedValue({ accepted: 1 })
    const { store } = show(true, 'inbox', { new: 1 })
    await screen.findByRole('tab', { name: 'New senders 1' })
    expect(screen.queryByText('Accept all')).toBeNull()
    store.set(inboxSectionAtom, 'new')
    fireEvent.click(await screen.findByText('Accept all'))
    await vi.waitFor(() => expect(wireAcceptSenders).toHaveBeenCalledWith(['eve@far.test']))
    await vi.waitFor(() => expect(invalidateMail).toHaveBeenCalled())
  })

  it('sits in the filter row from xl and on its own row below it', async () => {
    show(true, 'inbox', { reply: 1 }, () => (
      <>
        <FilterBar />
        <InboxSectionBar />
      </>
    ))
    await vi.waitFor(() =>
      expect(screen.getAllByRole('tablist', { name: 'Inbox sections' })).toHaveLength(2)
    )
    const bars = screen.getAllByRole('tablist', { name: 'Inbox sections' })
    const filterButton = screen.getByRole('button', { name: 'Show or hide filters' })
    const filterRow = filterButton.parentElement?.parentElement as HTMLElement
    const [inRow, own] = bars
    const inRowBox = inRow.parentElement as HTMLElement
    expect(inRowBox.parentElement).toBe(filterRow)
    expect(inRowBox.className.split(' ')).toEqual(
      expect.arrayContaining(['hidden', 'folders:flex'])
    )
    const ownBox = own.parentElement as HTMLElement
    expect(filterRow.contains(ownBox)).toBe(false)
    expect(ownBox.className.split(' ')).toContain('folders:hidden')
  })
})
