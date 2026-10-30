// "Search emails for: …" in the command palette searches the mail. It
// used to go to `/?q=`, which no page reads, so it searched nothing.

import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/hooks/use-sign-out', () => ({ useSignOut: () => vi.fn() }))

import { CommandPalette } from '@/components/command-palette'
import { searchQueryAtom } from '@/store/ui'

afterEach(cleanup)

describe('command palette search', () => {
  it('searches the mail for what was typed', async () => {
    const store = createStore()
    render(
      <Provider store={store}>
        <MemoryRouter initialEntries={['/settings']}>
          <CommandPalette />
          <Routes>
            <Route element={<p>mail page</p>} path="/mail" />
            <Route element={<p>settings page</p>} path="/settings" />
          </Routes>
        </MemoryRouter>
      </Provider>
    )
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', metaKey: true }))
    })
    const input = await screen.findByRole('combobox')
    fireEvent.change(input, { target: { value: '  invoice  ' } })
    fireEvent.click(screen.getByText('Search emails for: invoice'))
    expect(store.get(searchQueryAtom)).toBe('invoice')
    expect(screen.getByText('mail page')).toBeDefined()
  })
})
