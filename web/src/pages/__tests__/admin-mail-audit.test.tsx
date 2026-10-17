import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AdminMailAudit } from '../admin-mail-audit'

/** `GET /api/admin/audit/accounts`, as `audit_accounts` builds it. */
const ACCOUNTS = {
  items: [
    { active: true, address: 'ann@golia.jp', display_name: 'Ann', domain: 'golia.jp' },
    { active: false, address: 'old@marspot.com', display_name: '', domain: 'marspot.com' },
  ],
}

function renderAt(url = '/') {
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[url]}>
        <AdminMailAudit />
      </MemoryRouter>
    </QueryClientProvider>
  )
}

function stub(routes: Record<string, unknown>) {
  const calls: string[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      calls.push(path)
      if (!(path in routes)) return new Response('{"error":"core unavailable"}', { status: 502 })
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

describe('<AdminMailAudit />', () => {
  it('says each read is written to the audit log', async () => {
    stub({ '/api/admin/audit/accounts': ACCOUNTS })
    renderAt()
    expect(
      await screen.findByText(/writes a row to the audit log under your address/)
    ).toBeVisible()
  })

  it('lists accounts in the shared table and opens one', async () => {
    const calls = stub({
      '/api/admin/audit/accounts': ACCOUNTS,
      '/api/admin/audit/conversations?user=ann%40golia.jp&limit=50': {
        items: [],
        user: 'ann@golia.jp',
      },
    })
    renderAt()
    const table = await screen.findByRole('table', { name: 'Accounts' })
    const inactive = within(table).getByText('old@marspot.com').closest('tr') as HTMLElement
    expect(within(inactive).getByText('Inactive')).toBeInTheDocument()
    expect(within(inactive).getByText('—')).toBeInTheDocument()
    const ann = within(table).getByText('ann@golia.jp').closest('tr') as HTMLElement
    fireEvent.click(within(ann).getByRole('button', { name: 'View mail' }))
    expect(await screen.findByText('No conversations yet')).toBeInTheDocument()
    expect(calls).toContain('/api/admin/audit/conversations?user=ann%40golia.jp&limit=50')
  })

  it('filters accounts by the search in the URL', async () => {
    stub({ '/api/admin/audit/accounts': ACCOUNTS })
    renderAt('/?q=nobody')
    expect(await screen.findByText('No accounts match "nobody"')).toBeInTheDocument()
  })

  it("names what failed when an account's conversations cannot be read", async () => {
    stub({ '/api/admin/audit/accounts': ACCOUNTS })
    renderAt('/?address=ann%40golia.jp')
    expect(await screen.findByText("Could not load ann@golia.jp's conversations")).toBeVisible()
  })

  it('names what failed when a conversation cannot be read', async () => {
    stub({ '/api/admin/audit/accounts': ACCOUNTS })
    renderAt('/?address=ann%40golia.jp&thread=th1')
    expect(await screen.findByText('Could not load this conversation')).toBeVisible()
    expect(screen.getByText(/the audit log records it under your address/)).toBeVisible()
  })
})
