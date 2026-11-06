// The domain buttons under the inbox in the icon rail name the whole
// domain. They showed its first three letters, a cut-off "gol" that also
// gave golia.jp and golia.ai the same name.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/hooks/use-current-mail-filters', () => ({ useCurrentUnreadCount: () => 0 }))

import { AppSidebar } from '@/components/app-sidebar'
import { authAtom } from '@/store/auth'
import { selectedDomainsAtom } from '@/store/ui'

afterEach(cleanup)

function rail(domains: string[]) {
  const store = createStore()
  store.set(authAtom, {
    accessible_domains: domains,
    address: 'me@golia.jp',
    display_name: 'Me',
    permissions: [],
    token: 't',
  } as never)
  render(
    <QueryClientProvider client={new QueryClient()}>
      <Provider store={store}>
        <MemoryRouter initialEntries={['/mail']}>
          <AppSidebar />
        </MemoryRouter>
      </Provider>
    </QueryClientProvider>
  )
  return store
}

describe('the domain buttons in the icon rail', () => {
  it('show each domain in full and tell two of one name apart', () => {
    rail(['golia.jp', 'golia.ai'])
    expect(screen.getByRole('button', { name: 'golia.jp' }).textContent).toBe('golia.jp')
    expect(screen.getByRole('button', { name: 'golia.ai' }).textContent).toBe('golia.ai')
  })

  it('read one domain, then all of them again', () => {
    const store = rail(['golia.jp', 'golia.ai'])
    const button = screen.getByRole('button', { name: 'golia.ai' })
    fireEvent.click(button)
    expect(store.get(selectedDomainsAtom)).toEqual(['golia.ai'])
    expect(button.getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(button)
    expect(store.get(selectedDomainsAtom)).toEqual([])
  })
})
