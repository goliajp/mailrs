import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AdminMock } from '../admin-mock'

/** The shape `GET /api/admin/mock` answers with, as `AdminOverview` serializes it. */
const OVERVIEW = {
  bytes: 877,
  domain: 'mock.golia.jp',
  dropped: 3,
  max_bytes: 2 * 1024 * 1024 * 1024,
  max_namespaces_per_user: 10,
  namespaces: [
    {
      bytes: 470,
      created_at: 1_792_206_288_897,
      dropped: 3,
      messages: 1,
      name: 'devops-check',
      owner: 'lihao@golia.jp',
      smtp_enabled: true,
      webhook_url: 'https://hooks.example/in',
    },
    {
      bytes: 407,
      created_at: 1_792_206_300_000,
      dropped: 0,
      messages: 1,
      name: 'other-app',
      owner: 'ann@golia.jp',
      smtp_enabled: false,
      webhook_url: null,
    },
  ],
  received: 12,
  rejected_unknown_ns: 4,
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

function stubFetch(status: number, body: unknown) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () =>
      Promise.resolve(
        new Response(JSON.stringify(body), {
          headers: { 'content-type': 'application/json' },
          status,
        })
      )
    )
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('<AdminMock />', () => {
  it('lists every owner’s namespaces with the store totals', async () => {
    stubFetch(200, OVERVIEW)
    renderPage()
    expect(await screen.findByText('devops-check')).toBeInTheDocument()
    expect(screen.getByText('other-app')).toBeInTheDocument()
    expect(screen.getByText('ann@golia.jp')).toBeInTheDocument()
    expect(screen.getByText('2 (2 owners)')).toBeInTheDocument()
    expect(screen.getByText('https://hooks.example/in')).toBeInTheDocument()
    expect(screen.getByText('4')).toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'Delete' })).toHaveLength(2)
  })

  it('says the feature is off when the deployment has no mock domain', async () => {
    stubFetch(200, { ...OVERVIEW, bytes: 0, domain: '', namespaces: [] })
    renderPage()
    expect(
      await screen.findByText('Mock mail is not enabled on this deployment')
    ).toBeInTheDocument()
    expect(screen.queryByRole('table')).toBeNull()
  })
})
