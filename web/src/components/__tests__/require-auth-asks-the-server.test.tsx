// A browser with a live session cookie and no stored profile is signed in.
// It used to be sent to the login page before the server was asked.

import { cleanup, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

const me = { answer: null as null | Record<string, unknown> }
vi.mock('@/wire/endpoints/auth', () => ({
  wireGetMe: () => {
    if (me.answer) return Promise.resolve(me.answer)
    return Promise.reject(new Error('401'))
  },
}))

import { RequireAuth } from '@/components/require-auth'

afterEach(cleanup)

function show() {
  render(
    <Provider store={createStore()}>
      <MemoryRouter initialEntries={['/mail']}>
        <Routes>
          <Route element={<p>login page</p>} path="/login" />
          <Route
            element={
              <RequireAuth>
                <p>mail page</p>
              </RequireAuth>
            }
            path="/mail"
          />
        </Routes>
      </MemoryRouter>
    </Provider>
  )
}

describe('signed-in check', () => {
  it('lets a live session in with no stored profile', async () => {
    me.answer = {
      accessible_domains: [],
      address: 'qa@golia.jp',
      display_name: 'QA',
      is_admin: false,
      is_reset_issuer: false,
      is_super: false,
      permissions: [],
    }
    show()
    expect(screen.queryByText('login page')).toBeNull()
    expect(await screen.findByText('mail page')).toBeDefined()
  })

  it('sends a browser with no session to the login page', async () => {
    me.answer = null
    show()
    expect(await screen.findByText('login page')).toBeDefined()
  })
})
