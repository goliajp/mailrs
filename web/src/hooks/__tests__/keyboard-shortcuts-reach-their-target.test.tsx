// The list shortcuts act on what is on screen without searching the page
// for it. `r` and `f` used to look for an editor the reply box no longer
// has and a button by its English text; `#` deleted with no confirmation
// while every other delete asked; `j`/`k` scrolled to `aria-selected`,
// which no row sets; `c`, which the command palette advertises, did
// nothing.

import { act, cleanup, render } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, describe, expect, it, vi } from 'vitest'

const selection = { id: 't2' as null | string }
const rows = [{ threadId: 't1' }, { threadId: 't2' }, { threadId: 't3' }]
const deleted: string[][] = []

vi.mock('@/hooks/use-current-list', () => ({
  useConversationRows: () => ({ rows: [] }),
  useCurrentListRows: () => rows,
  useSelectedThreadId: () => selection.id,
  useSelectThreadId: () => (id: null | string) => {
    selection.id = id
  },
}))
vi.mock('@/wire/endpoints/mutations', () => ({
  wireArchiveThread: vi.fn(() => Promise.resolve()),
  wireBatchMutation: vi.fn((action: string, ids: string[]) => {
    if (action === 'delete') deleted.push(ids)
    return Promise.resolve()
  }),
  wireMarkThreadRead: vi.fn(() => Promise.resolve()),
  wirePinThread: vi.fn(() => Promise.resolve()),
  wireStarThread: vi.fn(() => Promise.resolve()),
  wireUnarchiveThread: vi.fn(() => Promise.resolve()),
  wireUnpinThread: vi.fn(() => Promise.resolve()),
  wireUnstarThread: vi.fn(() => Promise.resolve()),
}))

import { useKeyboardNav } from '@/hooks/use-keyboard-nav'
import { composingNewAtom, threadShortcutAtom } from '@/store/ui'

function Harness() {
  useKeyboardNav()
  return (
    <ul>
      {rows.map((r) => (
        <li aria-current={r.threadId === selection.id} key={r.threadId}>
          {r.threadId}
        </li>
      ))}
    </ul>
  )
}

function press(key: string) {
  act(() => {
    window.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key }))
  })
}

function setup() {
  selection.id = 't2'
  deleted.length = 0
  const store = createStore()
  const view = render(
    <Provider store={store}>
      <Harness />
    </Provider>
  )
  return { store, view }
}

afterEach(cleanup)

describe('list shortcuts', () => {
  it('r, f and # ask the open thread rather than act on the page', () => {
    const { store } = setup()
    press('r')
    expect(store.get(threadShortcutAtom)).toEqual({ kind: 'reply', threadId: 't2' })
    press('f')
    expect(store.get(threadShortcutAtom)).toEqual({ kind: 'forward', threadId: 't2' })
    press('#')
    expect(store.get(threadShortcutAtom)).toEqual({ kind: 'delete', threadId: 't2' })
    expect(deleted).toEqual([])
  })

  it('c starts a new message, as the palette says', () => {
    const { store } = setup()
    press('c')
    expect(store.get(composingNewAtom)).toBe(true)
  })

  it('j scrolls the newly picked row into view', async () => {
    const scrolled: string[] = []
    // jsdom has no scrollIntoView
    HTMLElement.prototype.scrollIntoView = function (this: HTMLElement) {
      scrolled.push(this.textContent ?? '')
    }
    const { store, view } = setup()
    press('j')
    expect(selection.id).toBe('t3')
    view.rerender(
      <Provider store={store}>
        <Harness />
      </Provider>
    )
    await new Promise((r) => requestAnimationFrame(r))
    expect(scrolled).toEqual(['t3'])
  })
})
