// How to point Bitwarden at this server: both services, this site's own
// address as the server, and the account's domain for addy.io.

import { cleanup, render, screen } from '@testing-library/react'
import { createStore, Provider } from 'jotai'
import { afterEach, describe, expect, it } from 'vitest'

import { MaskedForwardersHelp } from '@/components/settings/masked-forwarders-help'
import { authAtom } from '@/store/auth'

afterEach(cleanup)

describe('MaskedForwardersHelp', () => {
  it('names both services, this server and the account domain', () => {
    const store = createStore()
    store.set(authAtom, {
      accessible_domains: [],
      address: 'me@golia.jp',
      display_name: 'Me',
      is_admin: false,
      is_super: false,
      permissions: [],
    })
    render(
      <Provider store={store}>
        <MaskedForwardersHelp />
      </Provider>
    )
    expect(screen.getByText('SimpleLogin')).toBeTruthy()
    expect(screen.getByText('addy.io')).toBeTruthy()
    expect(screen.getAllByText(window.location.origin)).toHaveLength(2)
    expect(screen.getByText('golia.jp')).toBeTruthy()
  })
})
