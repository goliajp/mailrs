// With a conversation open, the palette offers its actions from the action
// table, the way the row's menu does, and runs them the same way.

import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

const acted: Array<[string, string]> = []
vi.mock('@/hooks/use-sign-out', () => ({ useSignOut: () => vi.fn() }))
vi.mock('@/hooks/use-current-list', () => ({
  useConversationRows: () => ({
    rows: [
      {
        archived: false,
        flagged: true,
        pinned: false,
        snoozed_until: null,
        thread_id: 't1',
        unread_count: 2,
      },
    ],
  }),
  useSelectedThreadId: () => 't1',
}))
vi.mock('@/hooks/use-conversation-actions', () => ({
  useConversationActions: () => ({
    act: (t: string, a: string) => {
      acted.push([t, a])
      return Promise.resolve()
    },
  }),
}))

import { CommandPalette } from '@/components/command-palette'
import { threadShortcutAtom } from '@/store/ui'

afterEach(cleanup)

describe('command palette', () => {
  it('offers the open conversation its actions and runs them', async () => {
    const store = createStore()
    render(
      <Provider store={store}>
        <MemoryRouter>
          <CommandPalette />
        </MemoryRouter>
      </Provider>
    )
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', metaKey: true }))
    })
    await screen.findByRole('combobox')
    // unread and starred: read, unstar; not archived: archive
    for (const label of ['Mark as read', 'Unstar', 'Archive', 'Move to Trash']) {
      expect(screen.getByText(label)).toBeDefined()
    }
    fireEvent.click(screen.getByText('Archive'))
    expect(acted).toEqual([['t1', 'archive']])

    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', metaKey: true }))
    })
    await screen.findByRole('combobox')
    // the Trash keeps it, so it runs like archiving, without asking
    fireEvent.click(screen.getByText('Move to Trash'))
    expect(acted).toEqual([
      ['t1', 'archive'],
      ['t1', 'trash'],
    ])
    expect(store.get(threadShortcutAtom)).toBeNull()
  })
})
