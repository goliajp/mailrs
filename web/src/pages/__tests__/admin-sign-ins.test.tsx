import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AdminSignIns } from '../admin-sign-ins'

/** `GET /api/admin/auth-events`, as `list_auth_events` returns `LoginEvent` rows. */
const EVENTS = {
  items: [
    {
      account: 'ann@golia.jp',
      channel: 'imap',
      credential: 'password',
      occurred_at: 1_792_206_000,
      product: 'mailrs',
      result: 'locked',
      source_ip: '198.51.100.7',
    },
    {
      account: 'smstransfer@golia.jp',
      channel: 'api-key',
      credential: 'api-key',
      key_id: 'mk_f319c',
      occurred_at: 1_792_205_000,
      product: 'mailrs',
      result: 'ok',
      source_ip: '203.0.113.4',
    },
  ],
}

function renderAt(url: string) {
  const fetchMock = vi.fn(async () =>
    Promise.resolve(
      new Response(JSON.stringify(EVENTS), {
        headers: { 'content-type': 'application/json' },
        status: 200,
      })
    )
  )
  vi.stubGlobal('fetch', fetchMock)
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[url]}>
        <AdminSignIns />
      </MemoryRouter>
    </QueryClientProvider>
  )
  return fetchMock
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('<AdminSignIns />', () => {
  it('lists the facts the server returns, newest first', async () => {
    renderAt('/')
    const table = await screen.findByRole('table', { name: 'Sign-ins' })
    const rows = within(table).getAllByRole('row')
    expect(rows[1]).toHaveTextContent('ann@golia.jp')
    expect(rows[1]).toHaveTextContent('locked')
    expect(rows[2]).toHaveTextContent('mk_f319c')
  })

  it('sends the filters in the URL to the server', async () => {
    const fetchMock = renderAt('/?account=ann%40golia.jp&result=locked&channel=imap')
    await screen.findByRole('table', { name: 'Sign-ins' })
    const url = String((fetchMock.mock.calls[0] as unknown[])[0])
    expect(url).toContain('/api/admin/auth-events?')
    expect(url).toContain('account=ann%40golia.jp')
    expect(url).toContain('result=locked')
    expect(url).toContain('channel=imap')
    expect(screen.getByLabelText('Filter by result')).toHaveValue('locked')
  })
})
