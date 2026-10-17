import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AdminApps } from '../admin-apps'

/** What `list_apps` answers: scopes are a list, and no secret or digest. */
const APPS = {
  items: [
    {
      app_id: 'app_1',
      created_at: 1_792_206_288,
      id: 1,
      name: 'ci-bot',
      scopes: ['mail.read', 'mail.send'],
    },
  ],
}
/** The prod capture of `GET /api/admin/permissions`, shortened. */
const PERMISSIONS = { permissions: ['mail.send', 'mail.read', 'admin.domains'] }

type Call = { body: unknown; method: string; path: string }

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <AdminApps />
      </MemoryRouter>
    </QueryClientProvider>
  )
}

function stub(routes: Record<string, { body: unknown; status: number }>): Call[] {
  const calls: Call[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      const method = init?.method ?? 'GET'
      calls.push({ body: init?.body && JSON.parse(String(init.body)), method, path })
      const r = routes[`${method} ${path}`] ?? { body: {}, status: 500 }
      if (r.status === 204) return new Response(null, { status: 204 })
      return new Response(JSON.stringify(r.body), {
        headers: { 'content-type': 'application/json' },
        status: r.status,
      })
    })
  )
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('<AdminApps />', () => {
  it('renders the scope list the backend sends', async () => {
    stub({
      'GET /api/admin/apps': { body: APPS, status: 200 },
      'GET /api/admin/permissions': { body: PERMISSIONS, status: 200 },
    })
    renderPage()
    const row = (await screen.findByText('ci-bot')).closest('tr') as HTMLElement
    expect(within(row).getByText('mail.read')).toBeInTheDocument()
    expect(within(row).getByText('mail.send')).toBeInTheDocument()
    expect(within(row).getByText('app_1')).toBeInTheDocument()
    expect(screen.queryByText('Owner')).toBeNull()
  })

  it('creates with a scope array and shows the secret once', async () => {
    const calls = stub({
      'GET /api/admin/apps': { body: APPS, status: 200 },
      'GET /api/admin/permissions': { body: PERMISSIONS, status: 200 },
      'POST /api/admin/apps': { body: { app_id: 'app_2', id: 2, secret: 'f00dcafe' }, status: 200 },
    })
    renderPage()
    await screen.findByText('ci-bot')
    fireEvent.click(screen.getByRole('button', { name: 'Add App' }))
    fireEvent.change(screen.getByLabelText('App name'), { target: { value: 'deploy-bot' } })
    fireEvent.click(await screen.findByLabelText('admin.domains'))
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    expect(await screen.findByText('f00dcafe')).toBeInTheDocument()
    expect(screen.getByText('Secret for app_2')).toBeInTheDocument()
    expect(screen.getByText(/Shown once/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Copy' })).toBeInTheDocument()
    const post = calls.find((c) => c.method === 'POST')
    expect(post?.body).toEqual({ name: 'deploy-bot', scopes: ['admin.domains'] })
  })

  it('saves edited scopes as an array and accepts 204', async () => {
    const calls = stub({
      'GET /api/admin/apps': { body: APPS, status: 200 },
      'GET /api/admin/permissions': { body: PERMISSIONS, status: 200 },
      'PUT /api/admin/apps/app_1/scopes': { body: null, status: 204 },
    })
    renderPage()
    await screen.findByText('ci-bot')
    fireEvent.click(screen.getByRole('button', { name: 'Scopes' }))
    const send = await screen.findByLabelText('mail.send')
    expect(send).toBeChecked()
    fireEvent.click(send)
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await vi.waitFor(() => expect(screen.queryByText('Edit Scopes')).toBeNull())
    const put = calls.find((c) => c.method === 'PUT')
    expect(put?.body).toEqual({ scopes: ['mail.read'] })
  })
})
