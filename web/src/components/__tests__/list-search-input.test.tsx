import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, describe, expect, it } from 'vitest'

import { ListSearchInput } from '@/components/list-search-input'
import { searchQueryAtom } from '@/store/ui'

afterEach(cleanup)

describe('ListSearchInput', () => {
  /**
   * The lists are mutually exclusive on screen, so this renders two of
   * them at once to say the thing that matters directly: they are one
   * box, not two that happen to look alike. Draft and Send each had a
   * private atom until 2026-08-05, and the query you had typed vanished
   * when you changed tab.
   */
  it('is one question no matter which list is drawing it', () => {
    const store = createStore()
    render(
      <Provider store={store}>
        <ListSearchInput label="Search conversations" />
        <ListSearchInput label="Search drafts" />
      </Provider>
    )

    fireEvent.change(screen.getByLabelText('Search conversations'), {
      target: { value: 'invoice' },
    })

    expect(screen.getByLabelText('Search drafts')).toHaveValue('invoice')
    expect(store.get(searchQueryAtom)).toBe('invoice')
  })

  it('clears the shared query', () => {
    const store = createStore()
    store.set(searchQueryAtom, 'invoice')
    render(
      <Provider store={store}>
        <ListSearchInput label="Search send" />
      </Provider>
    )

    fireEvent.click(screen.getByLabelText('Clear search'))
    expect(store.get(searchQueryAtom)).toBe('')
  })

  it('offers no clear button when there is nothing to clear', () => {
    render(
      <Provider store={createStore()}>
        <ListSearchInput label="Search send" />
      </Provider>
    )
    expect(screen.queryByLabelText('Clear search')).toBeNull()
  })

  /**
   * The trailing controls are why the conversation list hand-rolled a
   * byte-identical copy of this markup rather than using it.
   */
  it('renders trailing controls inside the header row', () => {
    render(
      <Provider store={createStore()}>
        <ListSearchInput label="Search conversations">
          <button aria-label="New conversation" />
        </ListSearchInput>
      </Provider>
    )
    expect(screen.getByLabelText('New conversation')).toBeTruthy()
  })
})
