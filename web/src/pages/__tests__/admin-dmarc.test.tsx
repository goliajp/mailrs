import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AdminDmarc } from '../admin-dmarc'

/** `list_sources` and `list_reports` as the handlers serialize them (snake_case). */
const SOURCES = {
  items: [
    { domains: ['golia.jp'], passing: 40, source_ip: '52.195.89.111', total: 40 },
    { domains: ['golia.jp'], passing: 0, source_ip: '198.51.100.7', total: 3 },
  ],
  passing: 40,
  reports: 2,
  total: 43,
}
const REPORTS = {
  items: [
    {
      begin: 1_792_108_800,
      email: 'noreply-dmarc-support@google.com',
      end: 1_792_195_199,
      org_name: 'google.com',
      p: 'none',
      passing: 40,
      policy_domain: 'golia.jp',
      rows: 2,
      sid: 'google.com!123',
      total: 43,
    },
  ],
}

type Call = { path: string }

function renderAt(url = '/') {
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[url]}>
        <AdminDmarc />
      </MemoryRouter>
    </QueryClientProvider>
  )
}

function stub(routes: Record<string, unknown>) {
  const calls: Call[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      calls.push({ path })
      if (!(path in routes)) return new Response('{"error":"boom"}', { status: 500 })
      return new Response(JSON.stringify(routes[path]), {
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

describe('<AdminDmarc />', () => {
  it('lists sources and reports with their pass rates', async () => {
    stub({
      '/api/admin/dmarc/reports?limit=50': REPORTS,
      '/api/admin/dmarc/sources?days=30': SOURCES,
    })
    renderAt()
    const spoofRow = (await screen.findByText('198.51.100.7')).closest('tr') as HTMLElement
    expect(within(spoofRow).getByText('0.0%')).toBeInTheDocument()
    const report = (await screen.findByText('google.com')).closest('tr') as HTMLElement
    expect(within(report).getByText('93.0%')).toBeInTheDocument()
  })

  it('keeps the window in the URL and asks for it', async () => {
    const calls = stub({
      '/api/admin/dmarc/reports?limit=50': REPORTS,
      '/api/admin/dmarc/sources?days=7': { ...SOURCES, items: [], reports: 0 },
      '/api/admin/dmarc/sources?days=30': SOURCES,
    })
    renderAt()
    await screen.findByText('198.51.100.7')
    fireEvent.click(screen.getByRole('button', { name: '7d' }))
    expect(await screen.findByText('No sending sources in the last 7 days')).toBeInTheDocument()
    expect(calls.some((c) => c.path === '/api/admin/dmarc/sources?days=7')).toBe(true)
  })

  it('reports a failed report list instead of calling it empty', async () => {
    stub({ '/api/admin/dmarc/sources?days=30': SOURCES })
    renderAt()
    expect(await screen.findByText('Could not load the report list')).toBeInTheDocument()
    expect(screen.queryByText('No DMARC reports yet')).toBeNull()
  })

  it('invites setting rua= when nothing has arrived', async () => {
    stub({
      '/api/admin/dmarc/reports?limit=50': { items: [] },
      '/api/admin/dmarc/sources?days=30': { items: [], passing: 0, reports: 0, total: 0 },
    })
    renderAt()
    expect(await screen.findByText('No DMARC reports yet')).toBeInTheDocument()
    expect(screen.getByText('No sending sources yet')).toBeInTheDocument()
  })
})
