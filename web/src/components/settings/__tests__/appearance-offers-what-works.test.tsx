// Appearance offered "Page size", which nothing read: lists load the next
// page as they scroll, at a size the client fixes. A control that does
// nothing is gone rather than kept.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, describe, expect, it, vi } from 'vitest'

// the swipe card on this page reads its setting from the server
vi.mock('@/wire/endpoints/mail-settings', () => ({
  wireGetSwipe: vi.fn(() => Promise.resolve({ left: 'trash', right: 'archive' })),
  wireSetSwipe: vi.fn(),
}))

import { AppearanceSection } from '../appearance-section'

afterEach(cleanup)

describe('appearance settings', () => {
  it('offers no page size', () => {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <Provider store={createStore()}>
          <AppearanceSection />
        </Provider>
      </QueryClientProvider>
    )
    expect(screen.getByText('Light')).toBeDefined()
    expect(screen.queryByLabelText('Page size')).toBeNull()
  })
})
