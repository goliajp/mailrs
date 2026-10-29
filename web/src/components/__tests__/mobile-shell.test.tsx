import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { MobileShell } from '@/components/mobile-shell'
import { authAtom } from '@/store/auth'

vi.mock('@/hooks/use-current-mail-filters', () => ({ useCurrentUnreadCount: () => 0 }))

function renderShell(isAdmin = false, isSuper = isAdmin) {
  const store = createStore()
  store.set(authAtom, {
    accessible_domains: [],
    address: 'u@example.com',
    display_name: 'U',
    is_admin: isAdmin,
    is_super: isSuper,
    permissions: [],
  })
  return render(
    <Provider store={store}>
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter>
          <MobileShell>
            <div>body</div>
          </MobileShell>
        </MemoryRouter>
      </QueryClientProvider>
    </Provider>
  )
}

function shellEl(): HTMLElement {
  return screen.getByText('body').closest('div.flex.flex-col') as HTMLElement
}

/**
 * A stand-in for the visual viewport, which jsdom does not implement.
 *
 * Only the height matters here: the shell's job is to be as tall as the
 * space the keyboard leaves, and that is a style value, not a layout
 * outcome — jsdom can answer it.
 */
function withViewport(height: number) {
  const listeners = new Set<() => void>()
  Object.defineProperty(window, 'visualViewport', {
    configurable: true,
    value: {
      height,
      addEventListener: (_: string, fn: () => void) => listeners.add(fn),
      removeEventListener: (_: string, fn: () => void) => listeners.delete(fn),
    },
  })
  return listeners
}

afterEach(cleanup)

describe('MobileShell height', () => {
  it('is the full screen when no keyboard is up', () => {
    window.innerHeight = 844
    withViewport(844)
    renderShell()
    expect(shellEl().style.height).toBe('100dvh')
  })

  /**
   * The bug this exists for: iOS does not shrink the layout viewport for
   * the keyboard, so at `100dvh` the bottom of this column — the nav, and
   * on the reply screen the Send button — is drawn underneath it, with
   * `overflow: hidden` on the document leaving nothing to scroll.
   */
  it('shrinks to what the keyboard leaves', () => {
    window.innerHeight = 844
    withViewport(500)
    renderShell()
    expect(shellEl().style.height).toBe('500px')
  })

  it('ignores a change too small to be a keyboard', () => {
    // The address bar collapsing is ~60px and must not resize the app.
    window.innerHeight = 844
    withViewport(790)
    renderShell()
    expect(shellEl().style.height).toBe('100dvh')
  })
})

// the server pages show other people's mail traffic, so the tab is for a
// super administrator only; offering it to anyone else leads to an error page
describe('MobileShell admin tab', () => {
  it('is not offered to an account that is not an administrator', () => {
    withViewport(844)
    renderShell(false)
    expect(screen.queryByText('Admin')).toBeNull()
    expect(screen.getByText('Settings')).toBeTruthy()
  })

  it('is not offered to an administrator who is not a super administrator', () => {
    withViewport(844)
    renderShell(true, false)
    expect(screen.queryByText('Admin')).toBeNull()
  })

  it('is offered to a super administrator', () => {
    withViewport(844)
    renderShell(true, true)
    expect(screen.getByText('Admin')).toBeTruthy()
  })
})
