import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { authAtom } from '@/store/auth'
import { composerWindowsAtom, openNewMessageAtom } from '@/store/composer-windows'
import { composingNewAtom } from '@/store/ui'

import { FloatingComposers } from '../floating-composers'

vi.mock('@/lib/api', () => ({
  deleteDraft: vi.fn(() => Promise.resolve({ success: true })),
  listDrafts: vi.fn(() => Promise.resolve([])),
  saveDraft: vi.fn(() => Promise.resolve({ id: 1, success: true })),
}))

afterEach(cleanup)

function renderWindows(count: number, pageOpen = false) {
  const store = createStore()
  store.set(composingNewAtom, pageOpen)
  store.set(authAtom, {
    accessible_domains: [],
    address: 'user@example.com',
    display_name: 'U',
    is_admin: false,
    is_super: false,
    permissions: [],
  })
  for (let i = 0; i < count; i++) store.set(openNewMessageAtom)
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <Provider store={store}>
        <MemoryRouter>
          <FloatingComposers />
        </MemoryRouter>
      </Provider>
    </QueryClientProvider>
  )
  return store
}

describe('floating composers', () => {
  it('gives each window its own message', async () => {
    renderWindows(2)
    const windows = screen.getAllByRole('dialog')
    expect(windows).toHaveLength(2)
    fireEvent.change(within(windows[0]).getByLabelText('Subject'), { target: { value: 'One' } })
    fireEvent.change(within(windows[1]).getByLabelText('Subject'), { target: { value: 'Two' } })
    expect(screen.getByRole('dialog', { name: 'One' })).toBeDefined()
    expect(screen.getByRole('dialog', { name: 'Two' })).toBeDefined()
  })

  it('keeps what was typed through folding and unfolding', async () => {
    const store = renderWindows(1)
    const win = screen.getByRole('dialog')
    fireEvent.change(within(win).getByLabelText('To'), { target: { value: 'bob@example.com' } })
    const body = (await within(win).findByPlaceholderText(
      'Write your message…',
      {},
      { timeout: 5_000 }
    )) as HTMLTextAreaElement
    fireEvent.change(body, { target: { value: 'hello bob' } })
    fireEvent.click(within(win).getByRole('button', { name: 'Minimize' }))
    expect(store.get(composerWindowsAtom)[0].minimized).toBe(true)
    fireEvent.click(within(win).getByRole('button', { name: 'Restore' }))
    expect((within(win).getByLabelText('To') as HTMLInputElement).value).toBe('bob@example.com')
    expect(
      (within(win).getByPlaceholderText('Write your message…') as HTMLTextAreaElement).value
    ).toBe('hello bob')
  })

  it('closes a window', () => {
    const store = renderWindows(1)
    fireEvent.click(within(screen.getByRole('dialog')).getAllByRole('button', { name: 'Close' })[0])
    expect(store.get(composerWindowsAtom)).toHaveLength(0)
  })

  // the page holds one message; replacing it would lose what it has typed
  // since its last autosave
  it('offers no expanding while a message is open on the page', () => {
    renderWindows(1, true)
    const expand = within(screen.getByRole('dialog')).getByRole('button', {
      name: 'Close the message open on the page first',
    })
    expect((expand as HTMLButtonElement).disabled).toBe(true)
  })

  it('expands when the page is free', () => {
    renderWindows(1)
    const expand = within(screen.getByRole('dialog')).getByRole('button', {
      name: 'Open as a full page',
    })
    expect((expand as HTMLButtonElement).disabled).toBe(false)
  })
})
