// Protocol sign-ins: each account's use of its own password and of app
// passwords, and its setting, which can be changed from the row.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/wire/endpoints/protocol-logins', () => ({
  wireProtocolLogins: vi.fn(),
  wireSetProtocolPassword: vi.fn(),
}))

import { AdminProtocolLogins } from '@/pages/admin-protocol-logins'
import { wireProtocolLogins, wireSetProtocolPassword } from '@/wire/endpoints/protocol-logins'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

function show() {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <AdminProtocolLogins />
    </QueryClientProvider>
  )
}

describe('protocol sign-ins', () => {
  it('lists use per protocol and changes an account setting', async () => {
    vi.mocked(wireProtocolLogins).mockResolvedValue({
      days: 30,
      default: 'allow',
      items: [
        {
          account: 'bob@golia.jp',
          app_password_last: null,
          app_password_logins: 0,
          channel: 'imap',
          password_last: 1_800_000_000,
          password_logins: 12,
        },
      ],
      policies: { 'bob@golia.jp': 'inherit' },
    })
    vi.mocked(wireSetProtocolPassword).mockResolvedValue()
    show()
    expect(await screen.findByText('bob@golia.jp')).toBeDefined()
    expect(screen.getByText(/^12 · last/)).toBeDefined()
    expect(screen.getByText('—')).toBeDefined()
    fireEvent.change(screen.getByLabelText('Setting for bob@golia.jp'), {
      target: { value: 'app-only' },
    })
    await vi.waitFor(() =>
      expect(wireSetProtocolPassword).toHaveBeenCalledWith('bob@golia.jp', 'app-only')
    )
  })

  it('says when no mail app signed in', async () => {
    vi.mocked(wireProtocolLogins).mockResolvedValue({
      days: 30,
      default: 'allow',
      items: [],
      policies: {},
    })
    show()
    expect(await screen.findByText('No protocol sign-ins')).toBeDefined()
  })
})
