import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { confirmPhrase, parseAddresses } from '@/lib/password-clear'

import { AdminPasswordClear } from '../admin-password-clear'

/** `POST /api/admin/password-clear/preview`, as `preview` answers it. */
const PREVIEW = {
  issuers_configured: false,
  items: [
    {
      address: 'ws@golia.jp',
      agent_keys: [],
      cleared_at: null,
      exists: true,
      external_logins: 0,
      sessions: 0,
      smtp_password_logins_7d: 0,
      totp: false,
    },
    {
      address: 'sentori@golia.jp',
      agent_keys: [{ id: 3, name: 'sentori', prefix: 'mk_75fe4', scopes: ['mail:send'] }],
      cleared_at: null,
      exists: true,
      external_logins: 0,
      sessions: 1,
      smtp_password_logins_7d: 30,
      totp: false,
    },
    {
      address: 'nobody@golia.jp',
      agent_keys: [],
      cleared_at: null,
      exists: false,
      external_logins: 0,
      sessions: 0,
      smtp_password_logins_7d: 0,
      totp: false,
    },
  ],
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

function renderPage() {
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <AdminPasswordClear />
    </QueryClientProvider>
  )
}

describe('parseAddresses and confirmPhrase', () => {
  it('takes one address per line or comma, once each, lowercased', () => {
    expect(parseAddresses(' WS@golia.jp\nws@golia.jp, roro@golia.jp;\n\n')).toEqual([
      'ws@golia.jp',
      'roro@golia.jp',
    ])
  })

  it('counts the accounts in what is typed', () => {
    expect(confirmPhrase(1)).toBe('clear 1 account')
    expect(confirmPhrase(7)).toBe('clear 7 accounts')
  })
})

describe('AdminPasswordClear', () => {
  it('previews, warns, and clears only once the count is typed', async () => {
    const fetchMock = vi.fn(async (url: string) => {
      if (url.endsWith('/preview')) return new Response(JSON.stringify(PREVIEW), { status: 200 })
      return new Response(
        JSON.stringify({
          items: [
            { address: 'ws@golia.jp', result: 'cleared', sessions_revoked: 0 },
            { address: 'sentori@golia.jp', result: 'cleared', sessions_revoked: 1 },
          ],
        }),
        { status: 200 }
      )
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    fireEvent.change(screen.getByLabelText('Accounts'), {
      target: { value: 'ws@golia.jp\nsentori@golia.jp\nnobody@golia.jp' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))

    await waitFor(() => expect(screen.getByText('sentori (mk_75fe4…)')).toBeTruthy())
    expect(screen.getByText(/MAILRS_RESET_ISSUERS is empty/)).toBeTruthy()
    expect(screen.getByText(/something runs on them/)).toBeTruthy()
    expect(screen.getByText('no such account')).toBeTruthy()

    // the account that does not exist is not counted
    const clear = screen.getByRole('button', { name: 'Clear 2 passwords' }) as HTMLButtonElement
    expect(clear.disabled).toBe(true)
    fireEvent.change(screen.getByLabelText('Confirmation'), {
      target: { value: 'clear 3 accounts' },
    })
    expect(clear.disabled).toBe(true)
    fireEvent.change(screen.getByLabelText('Confirmation'), {
      target: { value: 'clear 2 accounts' },
    })
    expect(clear.disabled).toBe(false)
    fireEvent.click(clear)

    await waitFor(() => expect(screen.getByText(/Cleared 2 accounts/)).toBeTruthy())
    const calls = fetchMock.mock.calls as unknown as [string, RequestInit][]
    const sent = calls.find(([u]) => u.endsWith('/admin/password-clear'))
    expect(JSON.parse(String(sent?.[1].body))).toEqual({
      addresses: ['ws@golia.jp', 'sentori@golia.jp'],
    })
  })
})
