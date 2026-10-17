import type { ReactNode } from 'react'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AdminAliases } from '../admin-aliases'
import { AdminEmailGroups } from '../admin-email-groups'
import { AdminGroups } from '../admin-groups'

function renderPage(page: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>{page}</MemoryRouter>
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

describe('<AdminAliases />', () => {
  it('shows what the store holds and deletes by source address', async () => {
    const calls = stub({
      'DELETE /api/admin/aliases/info%40bitreits.com': null,
      'GET /api/admin/aliases': {
        items: [
          {
            domain: 'bitreits.com',
            id: 'info@bitreits.com',
            source_address: 'info@bitreits.com',
            target_address: 'lihao@golia.jp',
          },
        ],
      },
    })
    renderPage(<AdminAliases />)
    expect(await screen.findByText('info@bitreits.com')).toBeInTheDocument()
    expect(screen.queryByText('Type')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
    const buttons = await screen.findAllByRole('button', { name: 'Delete' })
    fireEvent.click(buttons[buttons.length - 1])
    await vi.waitFor(() => expect(calls.some((c) => c.method === 'DELETE')).toBe(true))
    expect(calls.find((c) => c.method === 'DELETE')?.path).toBe(
      '/api/admin/aliases/info%40bitreits.com'
    )
  })

  it('creates an alias from the pair alone', async () => {
    const calls = stub({ 'GET /api/admin/aliases': { items: [] }, 'POST /api/admin/aliases': {} })
    renderPage(<AdminAliases />)
    fireEvent.click(await screen.findByRole('button', { name: 'Add Alias' }))
    expect(screen.queryByLabelText('Alias type')).toBeNull()
    fireEvent.change(screen.getByLabelText('Source address'), {
      target: { value: 'devops@golia.jp' },
    })
    fireEvent.change(screen.getByLabelText('Target address'), {
      target: { value: 'lihao@golia.jp' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await vi.waitFor(() => expect(calls.some((c) => c.method === 'POST')).toBe(true))
    expect(calls.find((c) => c.method === 'POST')?.body).toEqual({
      source_address: 'devops@golia.jp',
      target_address: 'lihao@golia.jp',
    })
  })
})

describe('<AdminGroups />', () => {
  it('reads permissions and members as string lists and labels the domain', async () => {
    stub({
      'GET /api/admin/domains': { items: [] },
      'GET /api/admin/groups': {
        items: [
          { description: '', domain: null, id: 1, is_builtin: false, name: 'everyone' },
          { description: '', id: 2, name: 'legacy' },
        ],
      },
      'GET /api/admin/groups/1/members': { members: ['qa@golia.jp'] },
      'GET /api/admin/groups/1/permissions': { permissions: ['admin.accounts'] },
      'GET /api/admin/permissions': { permissions: ['admin.accounts', 'admin.aliases'] },
    })
    renderPage(<AdminGroups />)
    expect(await screen.findByText('Global')).toBeInTheDocument()
    expect(screen.getByText('—')).toBeInTheDocument()
    expect(screen.queryByText('(Global)')).toBeNull()

    fireEvent.click(screen.getAllByRole('button', { name: 'Manage' })[0])
    expect(await screen.findByText('qa@golia.jp')).toBeInTheDocument()
    expect(screen.getByRole('checkbox', { name: 'admin.accounts' })).toBeChecked()
    expect(screen.getByRole('checkbox', { name: 'admin.aliases' })).not.toBeChecked()
  })
})

describe('<AdminEmailGroups />', () => {
  it('lists members from an {items} response', async () => {
    stub({
      'GET /api/admin/domains': { items: [] },
      'GET /api/admin/email-groups': {
        items: [
          {
            address: 'team@golia.jp',
            created_at: 0,
            description: '',
            domain: 'golia.jp',
            id: 3,
            name: 'Team',
          },
        ],
      },
      'GET /api/admin/email-groups/3/members': { items: ['qa@golia.jp'] },
    })
    renderPage(<AdminEmailGroups />)
    fireEvent.click(await screen.findByRole('button', { name: 'Members' }))
    expect(await screen.findByText('qa@golia.jp')).toBeInTheDocument()
  })
})
