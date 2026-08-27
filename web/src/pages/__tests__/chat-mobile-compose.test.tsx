import type { ReactNode } from 'react'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { MemoryRouter } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { authAtom } from '@/store/auth'
import { composingNewAtom } from '@/store/ui'

// Writing a mail is the whole point of the screen, and on a phone it
// was missing: `composingNew` was read only in the desktop branch, so
// the button, the shortcut, the palette and every draft row set the
// atom and rendered nothing.
let mobile = true
vi.mock('@/hooks/use-is-mobile', () => ({ useIsMobile: () => mobile }))
vi.mock('@/components/new-conversation', () => ({
  NewConversation: () => <div data-testid="composer">composer</div>,
}))
vi.mock('@/components/mobile-mail', () => ({ MobileMail: () => <div>reading</div> }))
vi.mock('@/components/conversation-list', () => ({
  ConversationList: () => <div>list</div>,
}))
vi.mock('@/hooks/use-mail-events', () => ({ useMailEvents: () => undefined }))
vi.mock('@/hooks/use-keyboard-nav', () => ({ useKeyboardNav: () => undefined }))

const { Chat } = await import('@/pages/chat')

function makeStore() {
  const store = createStore()
  store.set(authAtom, {
    accessible_domains: ['x.com'],
    address: 'me@x.com',
    display_name: 'Me',
    permissions: [],
    token: 't',
  })
  return store
}

function Wrapper({
  children,
  store,
}: {
  children: ReactNode
  store: ReturnType<typeof createStore>
}) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return (
    <QueryClientProvider client={qc}>
      <Provider store={store}>
        <MemoryRouter>{children}</MemoryRouter>
      </Provider>
    </QueryClientProvider>
  )
}

afterEach(cleanup)

describe('composing on a phone', () => {
  let store: ReturnType<typeof createStore>

  beforeEach(() => {
    store = makeStore()
    mobile = true
  })

  it('shows the composer when composing is on', () => {
    store.set(composingNewAtom, true)
    render(
      <Wrapper store={store}>
        <Chat />
      </Wrapper>
    )
    expect(screen.getByTestId('composer')).toBeDefined()
  })

  it('shows the list when it is not', () => {
    store.set(composingNewAtom, false)
    render(
      <Wrapper store={store}>
        <Chat />
      </Wrapper>
    )
    expect(screen.queryByTestId('composer')).toBeNull()
  })
})
