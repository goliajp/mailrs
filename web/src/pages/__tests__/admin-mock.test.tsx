import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AdminMock } from '../admin-mock'

/** `GET /api/admin/mock`, shaped as `AdminOverview` serializes. */
const OVERVIEW = {
  domain: 'mock.golia.jp',
  settings: {
    create: 'all-users',
    enabled: true,
    max_messages: 500,
    max_namespaces_per_user: 10,
    max_total_mb: 2048,
    retention_days: 3,
  },
  usage: { bytes: 877, dropped: 3, namespaces: 2, owners: 2, received: 12, rejected_unknown_ns: 4 },
}

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <AdminMock />
      </MemoryRouter>
    </QueryClientProvider>
  )
}

function stub(routes: Record<string, unknown>) {
  const calls: { body: unknown; method: string; path: string }[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      const method = init?.method ?? 'GET'
      calls.push({ body: init?.body && JSON.parse(String(init.body)), method, path })
      const body = routes[`${method} ${path}`]
      return Promise.resolve(
        new Response(JSON.stringify(body), {
          headers: { 'content-type': 'application/json' },
          status: body === undefined ? 500 : 200,
        })
      )
    })
  )
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('<AdminMock />', () => {
  it('shows usage and the saved settings', async () => {
    stub({ 'GET /api/admin/mock': OVERVIEW })
    renderPage()
    expect(await screen.findByText('2 owners')).toBeInTheDocument()
    expect(screen.getByText('4')).toBeInTheDocument()
    expect(screen.getByLabelText('Keep messages for (days)')).toHaveValue(3)
    expect(screen.getByRole('button', { name: 'Save settings' })).toBeDisabled()
  })

  it('saves a changed setting and refuses one out of range', async () => {
    const calls = stub({
      'GET /api/admin/mock': OVERVIEW,
      'PUT /api/admin/mock/settings': { ...OVERVIEW.settings, retention_days: 7 },
    })
    renderPage()
    const days = await screen.findByLabelText('Keep messages for (days)')
    fireEvent.change(days, { target: { value: '99' } })
    expect(screen.getByRole('alert')).toHaveTextContent('from 1 to 30')
    fireEvent.change(days, { target: { value: '7' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save settings' }))
    await vi.waitFor(() => expect(calls.some((c) => c.method === 'PUT')).toBe(true))
    const put = calls.find((c) => c.method === 'PUT')
    expect(put?.body).toEqual({ ...OVERVIEW.settings, retention_days: 7 })
  })

  it('says when no mock domain is configured', async () => {
    stub({ 'GET /api/admin/mock': { ...OVERVIEW, domain: '' } })
    renderPage()
    expect(await screen.findByText('No mock domain is configured')).toBeInTheDocument()
    expect(screen.queryByRole('form')).toBeNull()
  })
})
