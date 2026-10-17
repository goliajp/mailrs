import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AdminAuditLog } from '../admin-audit-log'

/** `GET /api/admin/audit-log?limit=200`, as `list_audit_log` returns `AuditRowWire` rows. */
const LOG = {
  items: [
    {
      action: 'audit.read_thread',
      actor: 'lihao@golia.jp',
      detail: 'th-81f2',
      id: 9,
      target: 'ann@golia.jp',
      timestamp: 1_792_206_000,
    },
    {
      action: 'system_config.set',
      actor: 'lihao@golia.jp',
      detail: '{"old":"52428800","new":"10485760"}',
      id: 8,
      target: 'max_message_size_bytes',
      timestamp: 1_792_205_000,
    },
    {
      action: 'auth.login',
      actor: 'ann@golia.jp',
      detail: '',
      id: 7,
      target: 'ann@golia.jp',
      timestamp: 1_792_204_000,
    },
  ],
}

function renderAt(url: string, status = 200, body: unknown = LOG) {
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
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[url]}>
        <AdminAuditLog />
      </MemoryRouter>
    </QueryClientProvider>
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('<AdminAuditLog />', () => {
  it('lists entries in the shared table, newest first', async () => {
    renderAt('/')
    const table = await screen.findByRole('table', { name: 'Audit log' })
    const rows = within(table).getAllByRole('row')
    expect(rows[1]).toHaveTextContent('audit.read_thread')
    expect(screen.getByText('3 of 3')).toBeInTheDocument()
  })

  it('reads its filters from the URL', async () => {
    renderAt('/?action=audit&actor=lihao')
    expect(await screen.findByText('audit.read_thread')).toBeInTheDocument()
    expect(screen.queryByText('auth.login')).toBeNull()
    expect(screen.queryByText('system_config.set')).toBeNull()
    expect(screen.getByLabelText('Filter by action')).toHaveValue('audit')
    expect(screen.getByLabelText('Filter by actor')).toHaveValue('lihao')
  })

  it('opens a JSON detail under its row', async () => {
    renderAt('/')
    const row = (await screen.findByText('system_config.set')).closest('tr') as HTMLElement
    fireEvent.click(within(row).getByRole('button', { name: 'Show detail' }))
    expect(screen.getByText(/"new": "10485760"/)).toBeInTheDocument()
    fireEvent.click(within(row).getByRole('button', { name: 'Hide detail' }))
    expect(screen.queryByText(/"new": "10485760"/)).toBeNull()
  })

  it('says when no entry matches the filters', async () => {
    renderAt('/?action=webhook')
    expect(await screen.findByText('No entries match these filters')).toBeInTheDocument()
  })

  it('invites rather than reports when the log is empty', async () => {
    renderAt('/', 200, { items: [] })
    expect(await screen.findByText('No audit entries yet')).toBeInTheDocument()
  })

  it('names what failed to load', async () => {
    renderAt('/', 500, { error: 'kevy unreachable' })
    expect(await screen.findByText('Could not load the audit log')).toBeInTheDocument()
  })
})
