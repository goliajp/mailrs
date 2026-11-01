// The sectioned inbox's bar: shown only for the Inbox with the setting on,
// switches the section, and accepts the senders of New.

import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/senders', () => ({ wireAcceptSenders: vi.fn() }))
vi.mock('@/lib/mail-cache', () => ({ invalidateMail: vi.fn() }))
vi.mock('@/hooks/use-current-list', () => ({
  useConversationRows: () => ({ rows: [{ participants: ['Eve <eve@far.test>'] }] }),
}))
vi.mock('@/hooks/use-open-list', () => ({ useOpenList: () => openList }))

const openList = vi.fn()

import { InboxSectionBar } from '@/components/inbox-section-bar'
import { invalidateMail } from '@/lib/mail-cache'
import { inboxSectionAtom, sectionedInboxAtom } from '@/store/inbox-sections'
import { activeListAtom } from '@/store/ui'
import { wireAcceptSenders } from '@/wire/endpoints/senders'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})

function show(sectioned: boolean, list: 'inbox' | 'unread' = 'inbox') {
  const store = createStore()
  store.set(sectionedInboxAtom, sectioned)
  store.set(activeListAtom, list)
  render(
    <Provider store={store}>
      <InboxSectionBar />
    </Provider>
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
})
