import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AdminDomains } from '../admin-domains'

function json(body: unknown) {
  return new Response(JSON.stringify(body), {
    headers: { 'content-type': 'application/json' },
    status: 200,
  })
}

function renderPage(domains: unknown[], hostname: null | string) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.includes('/admin/system-config')) {
        return json({ entries: [{ key: 'hostname', value: hostname }], success: true })
      }
      return json({ items: domains })
    })
  )
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <AdminDomains />
      </MemoryRouter>
    </QueryClientProvider>
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('<AdminDomains />', () => {
  it('lists the real steps, with the MX target this server answers on', async () => {
    renderPage([], 'mail.golia.ai')
    expect(await screen.findByText('No domains configured')).toBeInTheDocument()
    expect(
      await screen.findByText(
        'To receive mail for a domain: add it here, point its MX record at mail.golia.ai, then use Check to verify SPF, DKIM, DMARC and MX'
      )
    ).toBeInTheDocument()
  })

  it('does not invent a hostname it was not given', async () => {
    renderPage([], null)
    expect(
      await screen.findByText(/point its MX record at this server's mail hostname/)
    ).toBeInTheDocument()
  })

  it('keeps the steps above a list that is not empty', async () => {
    renderPage([{ created_at: 1_792_206_288, name: 'golia.jp' }], 'mail.golia.ai')
    expect(await screen.findByText('golia.jp')).toBeInTheDocument()
    expect(await screen.findByText(/accepted within a minute$/)).toBeInTheDocument()
  })
})
