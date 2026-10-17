import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AdminAccounts } from '../admin-accounts'

const QA = {
  active: true,
  address: 'qa@golia.jp',
  created_at: 1_772_571_432,
  display_name: 'QA',
  domain: 'golia.jp',
  quota_bytes: 10_737_418_240,
  recovery_email: '',
}

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <AdminAccounts />
      </MemoryRouter>
    </QueryClientProvider>
  )
}

/** `null` answers 204 with no body, as the admin writes do. */
function stub(routes: Record<string, unknown>) {
  const calls: { body: unknown; method: string; path: string }[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      const method = init?.method ?? 'GET'
      calls.push({ body: init?.body && JSON.parse(String(init.body)), method, path })
      const body = routes[`${method} ${path}`]
      if (body === null) return new Response(null, { status: 204 })
      if (body === undefined) return new Response('{}', { status: 500 })
      return new Response(JSON.stringify(body), {
        headers: { 'content-type': 'application/json' },
        status: 200,
      })
    })
  )
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('<AdminAccounts />', () => {
  it('says what deleting keeps, then deletes by address', async () => {
    const calls = stub({
      'DELETE /api/admin/accounts/qa%40golia.jp': null,
      'GET /api/admin/accounts': { items: [QA] },
      'GET /api/admin/accounts/qa%40golia.jp/quota': { quota_bytes: 10_737_418_240 },
    })
    renderPage()
    expect(await screen.findByText('qa@golia.jp')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
    expect(
      await screen.findByRole('alertdialog', { name: 'Delete qa@golia.jp?' })
    ).toHaveTextContent(
      'qa@golia.jp can no longer sign in, though sessions already open are not ended. Its stored mail stays on disk, and new mail to the address is still delivered into that mailbox. Its aliases and group memberships are kept'
    )
    fireEvent.click(screen.getByRole('button', { name: 'Delete account' }))
    await vi.waitFor(() => expect(calls.some((c) => c.method === 'DELETE')).toBe(true))
    expect(calls.find((c) => c.method === 'DELETE')?.path).toBe('/api/admin/accounts/qa%40golia.jp')
  })

  it('opens groups and sieve under the row, one at a time', async () => {
    stub({
      'GET /api/admin/accounts': { items: [QA] },
      'GET /api/admin/accounts/qa%40golia.jp/groups': { groups: ['ops'] },
      'GET /api/admin/accounts/qa%40golia.jp/quota': { quota_bytes: null },
      'GET /api/admin/accounts/qa%40golia.jp/sieve': { script: 'keep;' },
    })
    renderPage()
    fireEvent.click(await screen.findByRole('button', { name: 'Groups' }))
    expect(await screen.findByText('ops')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Sieve' }))
    expect(await screen.findByLabelText('Sieve script')).toHaveValue('keep;')
    expect(screen.queryByText('ops')).toBeNull()
  })

  it('confirms before deleting a sieve script', async () => {
    const calls = stub({
      'DELETE /api/admin/accounts/qa%40golia.jp/sieve': null,
      'GET /api/admin/accounts': { items: [QA] },
      'GET /api/admin/accounts/qa%40golia.jp/quota': { quota_bytes: null },
      'GET /api/admin/accounts/qa%40golia.jp/sieve': { script: 'keep;' },
    })
    renderPage()
    fireEvent.click(await screen.findByRole('button', { name: 'Sieve' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Delete script' }))
    const dialog = await screen.findByRole('alertdialog')
    expect(dialog).toHaveTextContent(
      'New mail to qa@golia.jp is no longer run through this script. Mail it already filed stays where it is'
    )
    const buttons = screen.getAllByRole('button', { name: 'Delete script' })
    fireEvent.click(buttons[buttons.length - 1])
    await vi.waitFor(() => expect(calls.some((c) => c.method === 'DELETE')).toBe(true))
    expect(calls.find((c) => c.method === 'DELETE')?.path).toBe(
      '/api/admin/accounts/qa%40golia.jp/sieve'
    )
  })

  it('invites the first account and names a failed load', async () => {
    stub({ 'GET /api/admin/accounts': { items: [] } })
    const { unmount } = renderPage()
    expect(await screen.findByText('No accounts yet')).toBeInTheDocument()
    unmount()

    stub({})
    renderPage()
    expect(await screen.findByText('Could not load the account list')).toBeInTheDocument()
  })

  it('creates an account from username and domain', async () => {
    const calls = stub({ 'GET /api/admin/accounts': { items: [] }, 'POST /api/admin/accounts': {} })
    renderPage()
    fireEvent.click(await screen.findByRole('button', { name: 'Add account' }))
    fireEvent.change(screen.getByLabelText('Username'), { target: { value: 'ann' } })
    fireEvent.change(screen.getByLabelText('Domain'), { target: { value: 'golia.jp' } })
    fireEvent.change(screen.getByLabelText('Password'), { target: { value: 'pw' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await vi.waitFor(() => expect(calls.some((c) => c.method === 'POST')).toBe(true))
    expect(calls.find((c) => c.method === 'POST')?.body).toEqual({
      address: 'ann@golia.jp',
      display_name: '',
      password: 'pw',
    })
  })
})
