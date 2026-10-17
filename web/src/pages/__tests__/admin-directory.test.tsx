import type { ReactNode } from 'react'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AdminAliases } from '../admin-aliases'
import { AdminEmailGroups } from '../admin-email-groups'
import { AdminGroups } from '../admin-groups'

const TEAM = {
  address: 'team@golia.jp',
  created_at: 1_792_206_288,
  description: 'Everyone at Golia',
  domain: 'golia.jp',
  id: 3,
  name: 'Team',
}

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
    const dialog = await screen.findByRole('alertdialog', { name: 'Delete info@bitreits.com?' })
    expect(dialog).toHaveTextContent(
      'Mail to info@bitreits.com stops going to lihao@golia.jp. If bitreits.com has a catch-all alias, that alias receives it instead. Mail already in lihao@golia.jp is kept'
    )
    fireEvent.click(screen.getByRole('button', { name: 'Delete alias' }))
    await vi.waitFor(() => expect(calls.some((c) => c.method === 'DELETE')).toBe(true))
    expect(calls.find((c) => c.method === 'DELETE')?.path).toBe(
      '/api/admin/aliases/info%40bitreits.com'
    )
  })

  it('says a catch-all covers the whole domain', async () => {
    stub({
      'GET /api/admin/aliases': {
        items: [
          {
            domain: 'golia.jp',
            id: '@golia.jp',
            source_address: '@golia.jp',
            target_address: 'lihao@golia.jp',
          },
        ],
      },
    })
    renderPage(<AdminAliases />)
    fireEvent.click(await screen.findByRole('button', { name: 'Delete' }))
    expect(await screen.findByRole('alertdialog')).toHaveTextContent(
      'Mail to addresses at golia.jp that have no mailbox or alias of their own stops going to lihao@golia.jp'
    )
  })

  it('invites the first alias when there is none', async () => {
    stub({ 'GET /api/admin/aliases': { items: [] } })
    renderPage(<AdminAliases />)
    expect(await screen.findByText('No aliases yet')).toBeInTheDocument()
  })

  it('names the list that failed to load', async () => {
    stub({})
    renderPage(<AdminAliases />)
    expect(await screen.findByText('Could not load the alias list')).toBeInTheDocument()
  })

  it('creates an alias from the pair alone', async () => {
    const calls = stub({ 'GET /api/admin/aliases': { items: [] }, 'POST /api/admin/aliases': {} })
    renderPage(<AdminAliases />)
    fireEvent.click(await screen.findByRole('button', { name: 'Add alias' }))
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
  it('reads permissions and members as string lists and claims no domain scope', async () => {
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
    expect(await screen.findByText('everyone')).toBeInTheDocument()
    // permissions are not limited to a domain, so the page offers no domain
    expect(screen.getByText(/on every domain/)).toBeInTheDocument()
    expect(screen.queryByText('Global')).toBeNull()
    expect(screen.queryByRole('columnheader', { name: 'Domain' })).toBeNull()

    fireEvent.click(screen.getAllByRole('button', { name: 'Manage' })[0])
    expect(await screen.findByText('qa@golia.jp')).toBeInTheDocument()
    expect(screen.getByRole('checkbox', { name: 'admin.accounts' })).toBeChecked()
    expect(screen.getByRole('checkbox', { name: 'admin.aliases' })).not.toBeChecked()
  })

  it('deletes a group after saying what its members lose', async () => {
    const calls = stub({
      'DELETE /api/admin/groups/1': null,
      'GET /api/admin/groups': {
        items: [
          { description: '', id: 1, is_builtin: false, name: 'ops' },
          { description: '', id: 2, is_builtin: true, name: 'admins' },
        ],
      },
      'GET /api/admin/permissions': { permissions: [] },
    })
    renderPage(<AdminGroups />)
    expect(await screen.findByText('ops')).toBeInTheDocument()
    // a built-in group offers no delete
    expect(screen.getAllByRole('button', { name: 'Delete' })).toHaveLength(1)
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
    expect(await screen.findByRole('alertdialog', { name: 'Delete ops?' })).toHaveTextContent(
      'Its members lose the permissions it granted, unless their own account or another group grants them too. The member accounts are kept'
    )
    fireEvent.click(screen.getByRole('button', { name: 'Delete group' }))
    await vi.waitFor(() => expect(calls.some((c) => c.method === 'DELETE')).toBe(true))
    expect(calls.find((c) => c.method === 'DELETE')?.path).toBe('/api/admin/groups/1')
  })

  it('removes a member through the confirm dialog', async () => {
    const calls = stub({
      'DELETE /api/admin/groups/1/members/qa%40golia.jp': null,
      'GET /api/admin/groups': { items: [{ description: '', id: 1, name: 'ops' }] },
      'GET /api/admin/groups/1/members': { members: ['qa@golia.jp'] },
      'GET /api/admin/groups/1/permissions': { permissions: [] },
      'GET /api/admin/permissions': { permissions: [] },
    })
    renderPage(<AdminGroups />)
    fireEvent.click(await screen.findByRole('button', { name: 'Manage' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Remove' }))
    expect(
      await screen.findByRole('alertdialog', { name: 'Remove qa@golia.jp from ops?' })
    ).toHaveTextContent('qa@golia.jp loses the permissions ops grants')
    fireEvent.click(screen.getByRole('button', { name: 'Remove member' }))
    await vi.waitFor(() => expect(calls.some((c) => c.method === 'DELETE')).toBe(true))
    expect(calls.find((c) => c.method === 'DELETE')?.path).toBe(
      '/api/admin/groups/1/members/qa%40golia.jp'
    )
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

  it('deletes a group after saying its address stops reaching the members', async () => {
    const calls = stub({
      'DELETE /api/admin/email-groups/3': null,
      'GET /api/admin/domains': { items: [] },
      'GET /api/admin/email-groups': { items: [TEAM] },
    })
    renderPage(<AdminEmailGroups />)
    fireEvent.click(await screen.findByRole('button', { name: 'Delete' }))
    expect(
      await screen.findByRole('alertdialog', { name: 'Delete team@golia.jp?' })
    ).toHaveTextContent(
      'Mail sent to team@golia.jp stops reaching its members, and the member list is deleted with the group. Mail the members already received is kept'
    )
    fireEvent.click(screen.getByRole('button', { name: 'Delete email group' }))
    await vi.waitFor(() => expect(calls.some((c) => c.method === 'DELETE')).toBe(true))
    expect(calls.find((c) => c.method === 'DELETE')?.path).toBe('/api/admin/email-groups/3')
  })

  it('removes a member through the confirm dialog', async () => {
    const calls = stub({
      'DELETE /api/admin/email-groups/3/members/ann%40example.com': null,
      'GET /api/admin/domains': { items: [] },
      'GET /api/admin/email-groups': { items: [TEAM] },
      'GET /api/admin/email-groups/3/members': { items: ['ann@example.com'] },
    })
    renderPage(<AdminEmailGroups />)
    fireEvent.click(await screen.findByRole('button', { name: 'Members' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Remove' }))
    expect(
      await screen.findByRole('alertdialog', { name: 'Remove ann@example.com from Team?' })
    ).toHaveTextContent(
      'ann@example.com stops receiving mail sent to team@golia.jp. Mail already delivered to it is kept'
    )
    fireEvent.click(screen.getByRole('button', { name: 'Remove member' }))
    await vi.waitFor(() => expect(calls.some((c) => c.method === 'DELETE')).toBe(true))
    expect(calls.find((c) => c.method === 'DELETE')?.path).toBe(
      '/api/admin/email-groups/3/members/ann%40example.com'
    )
  })
})
