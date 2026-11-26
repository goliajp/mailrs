import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// The batch wire call, so a test can say what was sent.
const batchCalls: Array<{ action: string; ids: string[] }> = []
vi.mock('@/wire/endpoints/mutations', () => ({
  wireBatchMutation: (action: string, ids: string[]) => {
    batchCalls.push({ action, ids })
    return Promise.resolve({ failed: 0, failed_thread_ids: [], message: 'Done', success: true })
  },
}))
vi.mock('@/lib/mail-cache', () => ({ invalidateBucketMove: vi.fn() }))

import { BatchActionBar } from '@/components/conversation-list-batch-action-bar'
import { useBatchSelection } from '@/hooks/use-batch-selection'
import { batchModeAtom, selectedThreadIdsAtom } from '@/store/ui'

type Row = { archived: boolean; flagged: boolean; thread_id: string; unread_count: number }

/** The bar as the list mounts it: on the list's own batch state. */
function Harness({ rows }: { rows: Row[] }) {
  const batch = useBatchSelection()
  return (
    <>
      <input aria-label="search" />
      {batch.picked.size > 0 && (
        <BatchActionBar
          loading={batch.loading}
          onAction={batch.run}
          onCancel={batch.exit}
          onLabel={() => {}}
          onToggleAll={() => batch.toggleAll(rows.map((r) => r.thread_id))}
          picked={batch.picked}
          rows={rows}
          view="other"
        />
      )}
    </>
  )
}

function row(id: string, unread = 0): Row {
  return { archived: false, flagged: false, thread_id: id, unread_count: unread }
}

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('the batch toolbar', () => {
  let store: ReturnType<typeof createStore>
  let rows: Row[]

  beforeEach(() => {
    store = createStore()
    rows = [row('t1'), row('t2')]
  })

  function renderPicked(ids: string[]) {
    batchCalls.length = 0
    store.set(batchModeAtom, true)
    store.set(selectedThreadIdsAtom, new Set(ids))
    render(
      <Provider store={store}>
        <Harness rows={rows} />
      </Provider>
    )
    return within(screen.getByRole('toolbar'))
  }

  describe('read or unread, whichever applies', () => {
    beforeEach(() => {
      rows = [row('t1', 2), row('t2')]
    })

    it('offers only "read" for an unread selection, and sends it', async () => {
      const bar = renderPicked(['t1'])
      expect(bar.queryByRole('button', { name: 'Mark as unread' })).toBeNull()
      fireEvent.click(bar.getByRole('button', { name: 'Mark as read' }))
      await waitFor(() => expect(batchCalls).toEqual([{ action: 'read', ids: ['t1'] }]))
    })

    it('offers only "unread" once nothing picked is unread', async () => {
      const bar = renderPicked(['t2'])
      expect(bar.queryByRole('button', { name: 'Mark as read' })).toBeNull()
      fireEvent.click(bar.getByRole('button', { name: 'Mark as unread' }))
      await waitFor(() => expect(batchCalls).toEqual([{ action: 'unread', ids: ['t2'] }]))
    })

    it('keeps "unread" in the menu when both kinds are picked', async () => {
      const bar = renderPicked(['t1', 't2'])
      expect(bar.getByRole('button', { name: 'Mark as read' })).toBeDefined()
      expect(bar.queryByRole('button', { name: 'Mark as unread' })).toBeNull()
      fireEvent.click(bar.getByRole('button', { name: 'More' }))
      fireEvent.click(bar.getByRole('menuitem', { name: /Mark as unread/ }))
      await waitFor(() => expect(batchCalls).toEqual([{ action: 'unread', ids: ['t1', 't2'] }]))
    })
  })

  describe('in a bar too narrow for every action', () => {
    // two 32 px slots and the gap between them: one action and the menu
    beforeEach(() => {
      vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(68)
    })

    it('draws what fits and puts the rest, in order, in the menu', () => {
      const bar = renderPicked(['t1'])
      expect(bar.getByRole('button', { name: 'Archive' })).toBeDefined()
      expect(bar.queryByRole('button', { name: 'Move to Trash' })).toBeNull()
      expect(bar.queryByRole('menu')).toBeNull()
      fireEvent.click(bar.getByRole('button', { name: 'More' }))
      const items = within(bar.getByRole('menu')).getAllByRole('menuitem')
      expect(items.map((i) => i.textContent)).toEqual([
        'Move to Trash#',
        'Mark as unreadu',
        'Stars',
        'Labelsl',
      ])
    })

    it('sends a menu action the way its button would, and closes the menu', async () => {
      const bar = renderPicked(['t1', 't2'])
      fireEvent.click(bar.getByRole('button', { name: 'More' }))
      fireEvent.click(bar.getByRole('menuitem', { name: /Move to Trash/ }))
      await waitFor(() => expect(batchCalls).toEqual([{ action: 'delete', ids: ['t1', 't2'] }]))
    })

    it('closes the menu on Escape before it leaves batch mode', () => {
      const bar = renderPicked(['t1'])
      fireEvent.click(bar.getByRole('button', { name: 'More' }))
      fireEvent.keyDown(window, { key: 'Escape' })
      expect(bar.queryByRole('menu')).toBeNull()
      expect(store.get(batchModeAtom)).toBe(true)
      fireEvent.keyDown(window, { key: 'Escape' })
      expect(store.get(batchModeAtom)).toBe(false)
    })
  })

  it('picks every loaded row from the box, and none once they all are', () => {
    const bar = renderPicked(['t1'])
    const box = bar.getByRole('checkbox', { name: 'Select all' })
    expect(box.getAttribute('aria-checked')).toBe('mixed')
    fireEvent.click(box)
    expect(store.get(selectedThreadIdsAtom)).toEqual(new Set(['t1', 't2']))
    expect(bar.getByText('2 selected')).toBeDefined()
    fireEvent.click(bar.getByRole('checkbox', { name: 'Clear selection' }))
    expect(store.get(selectedThreadIdsAtom).size).toBe(0)
  })

  it('runs an action key over the selection', async () => {
    renderPicked(['t1', 't2'])
    fireEvent.keyDown(window, { key: 'e' })
    await waitFor(() => expect(batchCalls).toEqual([{ action: 'archive', ids: ['t1', 't2'] }]))
  })

  it('leaves the keys alone while typing', () => {
    renderPicked(['t1'])
    fireEvent.keyDown(screen.getByLabelText('search'), { key: 'e' })
    expect(batchCalls).toEqual([])
  })
})
