/**
 * Signing out ends the session on the server or does not pretend to.
 *
 * The command palette used to clear the stored profile and go to /login
 * without telling the server, and the other two buttons ignored a failed
 * request. Either way the HttpOnly cookie kept working after the page said
 * the user was out.
 */

import type { ReactNode } from 'react'

import { act, renderHook } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { createElement } from 'react'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { PERSIST_KEY, queryClient } from '@/lib/query-client'
import { authAtom } from '@/store/auth'

const logout = vi.fn<() => Promise<void>>()
vi.mock('@/wire/endpoints/auth', () => ({ wireLogout: () => logout() }))
const toastError = vi.fn()
vi.mock('@goliapkg/gds', async () => {
  const real = await vi.importActual<Record<string, unknown>>('@goliapkg/gds')
  return { ...real, toast: { success: vi.fn(), error: (m: string) => toastError(m) } }
})

const { useSignOut } = await import('../use-sign-out')

const profile = {
  accessible_domains: [],
  address: 'a@x.com',
  display_name: 'A',
  is_admin: false,
  is_super: false,
  permissions: [],
}

function setup() {
  const store = createStore()
  store.set(authAtom, profile)
  queryClient.setQueryData(['mail', 'probe'], { subject: 'private' })
  localStorage.setItem(PERSIST_KEY, '{"clientState":{}}')
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(
      Provider,
      { store },
      createElement(
        MemoryRouter,
        { initialEntries: ['/mail'] },
        createElement(Routes, null, createElement(Route, { element: children, path: '*' }))
      )
    )
  const { result } = renderHook(() => ({ path: useLocation().pathname, signOut: useSignOut() }), {
    wrapper,
  })
  return { result, store }
}

describe('useSignOut', () => {
  beforeEach(() => {
    logout.mockReset()
    toastError.mockReset()
    queryClient.clear()
  })

  it('forgets the account only after the server ended the session', async () => {
    logout.mockResolvedValue(undefined)
    const { result, store } = setup()
    await act(() => result.current.signOut())
    expect(logout).toHaveBeenCalledOnce()
    expect(store.get(authAtom)).toBeNull()
    expect(queryClient.getQueryData(['mail', 'probe'])).toBeUndefined()
    expect(localStorage.getItem(PERSIST_KEY)).toBeNull()
    expect(result.current.path).toBe('/login')
  })

  it('stays signed in and says so when the server was not reached', async () => {
    logout.mockRejectedValue(new Error('offline'))
    const { result, store } = setup()
    await act(() => result.current.signOut())
    expect(store.get(authAtom)).toEqual(profile)
    expect(queryClient.getQueryData(['mail', 'probe'])).toEqual({ subject: 'private' })
    expect(result.current.path).toBe('/mail')
    expect(toastError).toHaveBeenCalledOnce()
  })
})
