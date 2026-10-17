import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AdminOverview } from '../admin-overview'

/**
 * The overview crashed twice in production on a wire shape it did not
 * expect, and once showed "0" for counts nobody had measured. These hold
 * both lines: odd shapes render, unknown renders as a dash.
 */

/** `GET /api/admin/flow` as `Flow` serializes (handlers/admin_flow.rs). */
const FLOW = {
  mock: { bytes: 877, max_bytes: 2_147_483_648, received: 5 },
  outbound: { failed: 0, queued: 2, retrying: 1, scheduled: 0 },
  receiving: { active_connections: 1, connections: 126, spooled: 10, uptime_secs: 11_060 },
  spool_backlog: 0,
}

function stubFetch(map: Record<string, unknown>): void {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
        .replace(/^\/api/, '')
        .replace(/\?.*$/, '')
      const body = map[path]
      if (body === undefined) return new Response('{"error":"no route"}', { status: 404 })
      return new Response(JSON.stringify(body), {
        headers: { 'content-type': 'application/json' },
        status: 200,
      })
    })
  )
}

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <AdminOverview />
      </MemoryRouter>
    </QueryClientProvider>
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('<AdminOverview />', () => {
  it('draws each stage of the flow with its count', async () => {
    stubFetch({
      '/admin/audit-log': { items: [] },
      '/admin/flow': FLOW,
      '/health': { kevy: true, uptime_secs: 60, version: '2.97.0' },
    })
    renderPage()
    const inbound = await screen.findByRole('list', { name: 'Inbound steps' })
    const outbound = screen.getByRole('list', { name: 'Outbound steps' })
    // a lane's steps are the list's own items; each holds a list of details
    expect(inbound.children).toHaveLength(3)
    expect(outbound.children).toHaveLength(3)
    expect(
      within(inbound).getByText('10 accepted in the 3h 4m since the receiver started')
    ).toBeInTheDocument()
    expect(within(outbound.children[0] as HTMLElement).getByText('2')).toBeInTheDocument()
    expect(screen.queryByRole('region', { name: 'Needs attention' })).toBeNull()
    expect(screen.getByText('Web API 2.97.0, up 1m')).toBeInTheDocument()
  })

  it('shows a dash for a stage nobody measured, and says what needs a person', async () => {
    stubFetch({
      '/admin/audit-log': { items: [] },
      '/admin/flow': { ...FLOW, outbound: { ...FLOW.outbound, failed: 4 }, receiving: null },
      '/health': { status: 'ok' },
    })
    renderPage()
    const inbound = await screen.findByRole('list', { name: 'Inbound steps' })
    expect(inbound.children[0]).toHaveTextContent('—')
    const attention = screen.getByRole('region', { name: 'Needs attention' })
    expect(attention).toHaveTextContent('4 outbound messages failed for good')
    expect(attention).toHaveTextContent('The receiver has not reported')
  })

  it('survives an audit log answer that is not a list', async () => {
    stubFetch({
      '/admin/audit-log': { error: 'unauthorized' },
      '/admin/flow': FLOW,
      '/health': { kevy: true },
    })
    renderPage()
    expect(await screen.findByRole('group', { name: 'Mail flow' })).toBeInTheDocument()
    expect(await screen.findByText(/The audit log could not be read/)).toBeInTheDocument()
  })
})
