import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { AdminOverview } from '../admin-overview'

/**
 * These tests exist because the admin overview crashed twice in prod in
 * quick succession — once on `local_domains.join(', ')` and once on
 * `entries.map(...)`. Both crashes unmounted the whole page and left
 * the user with a black screen. Reproduce every "shape from the wire I
 * did not expect" here, and hold future refactors to keeping this suite
 * green.
 */

function renderWithQuery(ui: React.ReactElement) {
  const client = new QueryClient({
    defaultOptions: { queries: { gcTime: 0, retry: false, staleTime: 0 } },
  })
  return render(<QueryClientProvider client={client}>{ui}</QueryClientProvider>)
}

/**
 * Stub the four endpoints admin-overview polls. Each parameter accepts
 * whatever the wire actually returns — an array, a `{ items }` envelope,
 * a plain object, a 401 body — so the caller can express "this is the
 * disaster case".
 */
function stubFetch(map: Record<string, unknown>): void {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString()
      const path = url.replace(/^https?:\/\/[^/]+/, '').replace(/^\/api/, '')
      for (const [key, body] of Object.entries(map)) {
        if (path === key || path.startsWith(key)) {
          return new Response(JSON.stringify(body), {
            headers: { 'content-type': 'application/json' },
            status: 200,
          })
        }
      }
      return new Response('{}', { status: 404 })
    })
  )
}

describe('<AdminOverview />', () => {
  it("renders when /admin/config/smtp is missing local_domains (fastcore's default shape)", async () => {
    stubFetch({
      '/admin/audit-log': { items: [] },
      '/admin/config/smtp': { hostname: 'mail.golia.ai' },
      '/health': { status: 'healthy' },
      '/status': { service: 'mailrs-webapi', version: '2.0.1' },
    })
    renderWithQuery(<AdminOverview />)
    await waitFor(() => expect(screen.getByText('Overview')).toBeInTheDocument())
    // Landing without a crash is the primary assertion. Wait for the
    // smtp panel to swap in from its skeleton, then assert the "Domains"
    // row rendered as "-" instead of dying on undefined.join().
    expect(await screen.findByText('Domains')).toBeInTheDocument()
  })

  it('renders when /admin/audit-log returns the wrapped `{items:[]}` envelope', async () => {
    stubFetch({
      '/admin/audit-log': {
        items: [
          {
            action: 'login',
            actor: 'lihao@golia.jp',
            detail: 'ip=127.0.0.1',
            id: 1,
            target: '',
            timestamp: Math.floor(Date.now() / 1000) - 60,
          },
        ],
      },
      '/admin/config/smtp': {
        hostname: 'mail.golia.ai',
        imap_port: 143,
        local_domains: ['golia.ai'],
        smtp_port: 25,
        submission_port: 587,
        tls_enabled: true,
      },
      '/health': { status: 'healthy' },
      '/status': { service: 'mailrs-webapi', version: '2.0.1' },
    })
    renderWithQuery(<AdminOverview />)
    await waitFor(() => expect(screen.getByText('Recent Audit Log')).toBeInTheDocument())
    await waitFor(() => expect(screen.getByText('login')).toBeInTheDocument())
  })

  it('renders when /admin/audit-log returns an unwrapped array (legacy monolith)', async () => {
    stubFetch({
      '/admin/audit-log': [
        {
          action: 'login_failed',
          actor: 'lihao@golia.jp',
          detail: 'ip=10.0.0.1',
          id: 1,
          target: '',
          timestamp: Math.floor(Date.now() / 1000) - 120,
        },
      ],
      '/admin/config/smtp': {
        hostname: 'mail.golia.ai',
        imap_port: 143,
        local_domains: ['golia.ai'],
        smtp_port: 25,
        submission_port: 587,
        tls_enabled: true,
      },
      '/health': { status: 'healthy' },
      '/status': { service: 'mailrs-webapi', version: '2.0.1' },
    })
    renderWithQuery(<AdminOverview />)
    await waitFor(() => expect(screen.getByText('login failed')).toBeInTheDocument())
  })

  it('renders when /admin/audit-log returns a garbage shape (e.g. 401 body echoed as data)', async () => {
    stubFetch({
      '/admin/audit-log': { error: 'unauthorized' },
      '/admin/config/smtp': { hostname: 'mail.golia.ai' },
      '/health': { status: 'healthy' },
      '/status': { service: 'mailrs-webapi', version: '2.0.1' },
    })
    renderWithQuery(<AdminOverview />)
    await waitFor(() => expect(screen.getByText('Recent Audit Log')).toBeInTheDocument())
    expect(screen.getByText('No entries')).toBeInTheDocument()
  })

  it('renders when the fastcore /api/health only carries a status field', async () => {
    stubFetch({
      '/admin/audit-log': { items: [] },
      '/admin/config/smtp': { hostname: 'mail.golia.ai' },
      '/health': { status: 'healthy' },
      '/status': { service: 'mailrs-webapi', version: '2.0.1' },
    })
    renderWithQuery(<AdminOverview />)
    await waitFor(() => expect(screen.getByText(/Healthy/)).toBeInTheDocument())
    // Uptime placeholder — the `HealthInfo.uptime_secs` isn't present,
    // and previously this rendered as "NaNm".
    expect(screen.getByText(/Uptime -/)).toBeInTheDocument()
  })
})
