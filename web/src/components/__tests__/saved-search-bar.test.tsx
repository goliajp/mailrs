// Saved searches: one click runs the query, the name saves what is in the
// box, and a search can be deleted.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/saved-searches', () => ({
  wireCreateSavedSearch: vi.fn(),
  wireDeleteSavedSearch: vi.fn(),
  wireListSavedSearches: vi.fn(),
}))

import { SavedSearchBar, SaveSearchButton } from '@/components/saved-search-bar'
import { searchQueryAtom } from '@/store/ui'
import {
  wireCreateSavedSearch,
  wireDeleteSavedSearch,
  wireListSavedSearches,
} from '@/wire/endpoints/saved-searches'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

const INVOICES = { created_at: 1, id: '1', name: 'Invoices', query: 'has:attachment invoice' }

function renderWith(node: React.ReactNode, query = '') {
  const store = createStore()
  store.set(searchQueryAtom, query)
  render(
    <QueryClientProvider client={new QueryClient()}>
      <Provider store={store}>{node}</Provider>
    </QueryClientProvider>
  )
  return store
}

describe('saved searches', () => {
  it('run their query when clicked', async () => {
    vi.mocked(wireListSavedSearches).mockResolvedValue([INVOICES])
    const store = renderWith(<SavedSearchBar />)
    fireEvent.click(await screen.findByText('Invoices'))
    expect(store.get(searchQueryAtom)).toBe('has:attachment invoice')
  })

  it('draw nothing when there are none', async () => {
    vi.mocked(wireListSavedSearches).mockResolvedValue([])
    renderWith(<SavedSearchBar />)
    await vi.waitFor(() => expect(wireListSavedSearches).toHaveBeenCalled())
    expect(screen.queryByRole('toolbar')).toBeNull()
  })

  it('can be deleted', async () => {
    vi.mocked(wireListSavedSearches).mockResolvedValue([INVOICES])
    vi.mocked(wireDeleteSavedSearch).mockResolvedValue()
    renderWith(<SavedSearchBar />)
    fireEvent.click(await screen.findByLabelText('Delete saved search Invoices'))
    await vi.waitFor(() => expect(wireDeleteSavedSearch).toHaveBeenCalledWith('1'))
  })

  it('save what is in the box under the name given', async () => {
    vi.mocked(wireCreateSavedSearch).mockResolvedValue(INVOICES)
    renderWith(<SaveSearchButton />, 'from:billing ')
    fireEvent.click(screen.getByLabelText('Save this search'))
    const name = screen.getByLabelText('Name')
    expect((name as HTMLInputElement).value).toBe('from:billing')
    fireEvent.change(name, { target: { value: 'Bills' } })
    fireEvent.click(screen.getByText('Save'))
    await vi.waitFor(() =>
      expect(wireCreateSavedSearch).toHaveBeenCalledWith('Bills', 'from:billing ')
    )
  })
})
