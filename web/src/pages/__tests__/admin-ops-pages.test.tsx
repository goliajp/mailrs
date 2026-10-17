import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { matchesActionFilter } from '../admin-audit-actions'
import { AdminAuditLog } from '../admin-audit-log'
import { AdminMailAudit } from '../admin-mail-audit'

type Call = { method: string; path: string }

function renderAt(ui: React.ReactElement, url = '/') {
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[url]}>{ui}</MemoryRouter>
    </QueryClientProvider>
  )
}

/** Answer `METHOD path` (query string included) from `routes`; anything else is a 404. */
function stub(routes: Record<string, unknown>) {
  const calls: Call[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      const method = init?.method ?? 'GET'
      calls.push({ method, path })
      const key = `${method} ${path}`
      if (!(key in routes)) return new Response('{}', { status: 404 })
      const body = routes[key]
      if (body === null) return new Response(null, { status: 204 })
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

describe('audit log action filters', () => {
  it('match the dotted actions the backend writes', () => {
    expect(matchesActionFilter('domain.create', 'domain')).toBe(true)
    expect(matchesActionFilter('auth.login.external', 'auth')).toBe(true)
    expect(matchesActionFilter('mail.send_failed', 'mail')).toBe(true)
    expect(matchesActionFilter('email_group.create', 'group')).toBe(false)
    expect(matchesActionFilter('email_group.create', 'email_group')).toBe(true)
    expect(matchesActionFilter('domain_create', 'domain')).toBe(false)
    for (const action of [
      'mock.settings_update',
      'mock.namespace_create',
      'mock.namespace_delete',
    ]) {
      expect(matchesActionFilter(action, 'mock')).toBe(true)
    }
    expect(matchesActionFilter('mock.namespace_create', 'mail')).toBe(false)
  })

  it('filters the rendered rows', async () => {
    stub({
      'GET /api/admin/audit-log?limit=200': {
        items: [
          {
            action: 'domain.create',
            actor: 'a@b.c',
            detail: '',
            id: 2,
            target: 'x.org',
            timestamp: 1,
          },
          {
            action: 'account.create',
            actor: 'a@b.c',
            detail: '',
            id: 1,
            target: 'u@x.org',
            timestamp: 1,
          },
        ],
      },
    })
    renderAt(<AdminAuditLog />)
    expect(await screen.findByText('account.create')).toBeInTheDocument()
    fireEvent.change(screen.getByLabelText('Filter by action'), { target: { value: 'domain' } })
    expect(screen.getByText('domain.create')).toBeInTheDocument()
    expect(screen.queryByText('account.create')).toBeNull()
  })
})

describe('<AdminMailAudit />', () => {
  it("lists the target user's conversations with participants as a string", async () => {
    const calls = stub({
      'GET /api/admin/audit/accounts': { items: [] },
      'GET /api/admin/audit/conversations?user=t%40b.c&limit=50': {
        items: [
          {
            last_date: 1,
            message_count: 2,
            participants: 'x@y.z, t@b.c',
            snippet: 'hello',
            subject: 'Invoice',
            thread_id: 'th1',
          },
        ],
        user: 't@b.c',
      },
    })
    renderAt(<AdminMailAudit />, '/?address=t%40b.c')
    expect(await screen.findByText('x@y.z, t@b.c')).toBeInTheDocument()
    expect(calls.some((c) => c.path.includes('target_user'))).toBe(false)
  })

  it("shows a message body and attachment names without links into the admin's own mailbox", async () => {
    stub({
      'GET /api/admin/audit/accounts': { items: [] },
      'GET /api/admin/audit/conversations/th1/messages?user=t%40b.c': {
        items: [
          {
            attachments: [{ content_type: 'application/pdf', filename: 'bill.pdf', size: 2048 }],
            html_body: null,
            internal_date: 1,
            recipients: 't@b.c',
            risk_score: 0,
            sender: 'x@y.z',
            subject: 'Invoice',
            text_body: 'please pay',
            uid: 7,
          },
        ],
      },
    })
    const { container } = renderAt(<AdminMailAudit />, '/?address=t%40b.c&thread=th1')
    expect(await screen.findByText('please pay')).toBeInTheDocument()
    expect(screen.getByText(/bill\.pdf/)).toBeInTheDocument()
    expect(container.querySelector('a[href*="/api/mail/messages"]')).toBeNull()
    const eml = screen.getByLabelText('Download as .eml')
    expect(eml.getAttribute('href')).toContain('/api/admin/audit/messages/7/raw?user=t%40b.c')
  })
})
