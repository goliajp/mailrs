// The list shortcuts act on what is on screen without searching the page,
// and each key does what the action table says on the conversation in its
// current state, through the same dispatcher as the mouse: `e` archives or
// puts back, `s` stars or unstars, `z` takes the last action back.

import { cleanup, render, act as rtlAct } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, describe, expect, it, vi } from 'vitest'

const selection = { id: 't2' as null | string }
const rows = [{ threadId: 't1' }, { threadId: 't2' }, { threadId: 't3' }]
const convo = {
  archived: false,
  flagged: true,
  pinned: false,
  snoozed_until: null as null | number,
  thread_id: 't2',
  unread_count: 0,
}
const acted: Array<[string, string]> = []

vi.mock('@/hooks/use-current-list', () => ({
  useConversationRows: () => ({ rows: [convo] }),
  useCurrentListRows: () => rows,
  useSelectedThreadId: () => selection.id,
  useSelectThreadId: () => (id: null | string) => {
    selection.id = id
  },
}))
vi.mock('@/hooks/use-conversation-actions', () => ({
  useConversationActions: () => ({
    act: (threadId: string, action: string) => {
      acted.push([threadId, action])
      return Promise.resolve()
    },
  }),
}))

import { useKeyboardNav } from '@/hooks/use-keyboard-nav'
import {
  batchModeAtom,
  composingNewAtom,
  lastUndoAtom,
  selectedThreadIdsAtom,
  threadShortcutAtom,
} from '@/store/ui'

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

function press(key: string, shiftKey = false, held: KeyboardEventInit = {}) {
  rtlAct(() => {
    window.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key, shiftKey, ...held }))
  })
}

function setup() {
  selection.id = 't2'
  acted.length = 0
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
    press('#', true)
    expect(store.get(threadShortcutAtom)).toEqual({ kind: 'delete', threadId: 't2' })
    expect(acted).toEqual([])
  })

  it('each key does what the table says on the conversation as it is', () => {
    setup()
    // starred and not archived: `s` unstars, `e` archives
    press('s')
    press('p')
    press('b')
    press('!', true)
    press('u')
    press('e')
    expect(acted).toEqual([
      ['t2', 'unstar'],
      ['t2', 'pin'],
      ['t2', 'snooze'],
      ['t2', 'mark-junk'],
      ['t2', 'unread'],
      ['t2', 'archive'],
    ])
    // archiving drops the pick; the list's first row becomes it
    expect(selection.id).toBeNull()
  })

  it('z takes back the last action, once', () => {
    const { store } = setup()
    rtlAct(() => store.set(lastUndoAtom, { action: 'unarchive', threadId: 't9' }))
    press('z')
    press('z')
    expect(acted).toEqual([['t9', 'unarchive']])
  })

  it('x picks the open conversation for a batch, and puts it back', () => {
    const { store } = setup()
    press('x')
    expect(store.get(batchModeAtom)).toBe(true)
    expect([...store.get(selectedThreadIdsAtom)]).toEqual(['t2'])
    press('x')
    expect([...store.get(selectedThreadIdsAtom)]).toEqual([])
  })

  it('c starts a new message, as the palette says', () => {
    const { store } = setup()
    press('c')
    expect(store.get(composingNewAtom)).toBe(true)
  })

  // Alt+C, Ctrl+C and Cmd+C are copy, not c
  it('a key held with Alt, Ctrl or Cmd is left to the browser', () => {
    const { store } = setup()
    for (const held of [{ altKey: true }, { ctrlKey: true }, { metaKey: true }]) {
      press('c', false, held)
      press('e', false, held)
    }
    expect(store.get(composingNewAtom)).toBe(false)
    expect(acted).toEqual([])
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
