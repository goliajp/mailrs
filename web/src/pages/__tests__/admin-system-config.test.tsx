import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AdminSystemConfig } from '../admin-system-config'

const NOTE =
  'Set by MAILRS_PORT on the receiver service in docker-compose and read when the process starts. Changing it means editing compose and restarting the container.'

/** Shaped as `get_system_config` builds it: two settings read at start, one live, one unread override. */
const CONFIG = {
  entries: [
    {
      default_value: '2525',
      description: 'Inbound SMTP port on the receiver',
      editable: false,
      env_var: 'MAILRS_PORT',
      group: 'smtp',
      key: 'smtp_port',
      note: NOTE,
      source: 'env',
      value: '25',
      value_type: 'string',
    },
    {
      default_value: null,
      description: 'TLS on 465 / 587 / 993 / 995 and STARTTLS on 25',
      editable: false,
      env_var: 'MAILRS_TLS_CERT, MAILRS_TLS_KEY',
      group: 'security',
      key: 'tls_enabled',
      note: 'Set by MAILRS_TLS_CERT, MAILRS_TLS_KEY on the receiver, fastcore service',
      source: 'unknown',
      value: null,
      value_type: 'string',
    },
    {
      default_value: '52428800',
      description: 'Largest message the receiver accepts, in bytes',
      editable: true,
      group: 'smtp',
      key: 'max_message_size_bytes',
      source: 'database',
      updated_at: 1_792_206_288,
      updated_by: 'lihao@golia.jp',
      value: '10485760',
      value_type: 'string',
    },
    {
      default_value: null,
      description: 'Stored here earlier, but no process reads this key',
      editable: false,
      group: 'custom',
      key: 'local_domains',
      note: 'Local domains are managed on the Domains page',
      source: 'database',
      value: 'golia.jp',
      value_type: 'string',
    },
  ],
  success: true,
}

function renderPage() {
  const fetchMock = vi.fn(async (_url: RequestInfo | URL, init?: RequestInit) => {
    if (init?.method === 'DELETE') return Promise.resolve(new Response(null, { status: 204 }))
    return Promise.resolve(
      new Response(JSON.stringify(CONFIG), {
        headers: { 'content-type': 'application/json' },
        status: 200,
      })
    )
  })
  vi.stubGlobal('fetch', fetchMock)
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <AdminSystemConfig />
      </MemoryRouter>
    </QueryClientProvider>
  )
  return fetchMock
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('<AdminSystemConfig />', () => {
  it('gives an input only to the setting a running process reads', async () => {
    renderPage()
    expect(await screen.findByText('SMTP')).toBeInTheDocument()
    expect(screen.getAllByRole('textbox')).toHaveLength(1)
    expect(screen.getByRole('textbox', { name: 'max_message_size_bytes' })).toHaveValue('10485760')
    expect(screen.getByText(/^Saved by lihao@golia\.jp on /)).toBeInTheDocument()
  })

  it('shows a setting read at start with where it is set and how to change it', async () => {
    renderPage()
    expect(await screen.findByTestId('readonly-smtp_port')).toHaveTextContent('25')
    expect(screen.getByText('MAILRS_PORT')).toBeInTheDocument()
    expect(screen.getByText(NOTE)).toBeInTheDocument()
    expect(screen.getAllByText('Read-only')).toHaveLength(3)
  })

  it('renders a value webapi cannot see as unknown, not as empty', async () => {
    renderPage()
    expect(await screen.findByTestId('readonly-tls_enabled')).toHaveTextContent('—')
    expect(screen.getByText('Not visible here')).toBeInTheDocument()
  })

  it('lets an override nothing reads be removed, and sends local domains to the Domains page', async () => {
    renderPage()
    expect(await screen.findByTestId('readonly-local_domains')).toHaveTextContent('golia.jp')
    expect(screen.getAllByRole('button', { name: 'Reset to default' })).toHaveLength(1)
    expect(screen.getAllByRole('button', { name: 'Remove' })).toHaveLength(1)
    expect(screen.getAllByText(/Domains page/).length).toBeGreaterThan(0)
  })

  it('asks before resetting, says what the receiver does, then sends the DELETE', async () => {
    const fetchMock = renderPage()
    fireEvent.click(await screen.findByRole('button', { name: 'Reset to default' }))
    const dialog = screen.getByRole('alertdialog', { name: 'Reset max_message_size_bytes?' })
    expect(dialog).toHaveTextContent(
      '10485760 is removed and the built-in 52428800 applies again. The receiver uses it for new connections within a minute'
    )
    expect(fetchMock.mock.calls.some(([, init]) => init?.method === 'DELETE')).toBe(false)
    fireEvent.click(within(dialog).getByRole('button', { name: 'Reset to default' }))
    await vi.waitFor(() =>
      expect(
        fetchMock.mock.calls.some(
          ([url, init]) =>
            init?.method === 'DELETE' &&
            String(url) === '/api/admin/system-config/max_message_size_bytes'
        )
      ).toBe(true)
    )
  })

  it('says removing a key nothing reads changes nothing', async () => {
    renderPage()
    fireEvent.click(await screen.findByRole('button', { name: 'Remove' }))
    expect(screen.getByRole('alertdialog', { name: 'Remove local_domains?' })).toHaveTextContent(
      'No process reads local_domains, so removing the stored value changes nothing on the server'
    )
  })

  it('names what failed to load', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => Promise.resolve(new Response('{"error":"kevy down"}', { status: 500 })))
    )
    const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <AdminSystemConfig />
        </MemoryRouter>
      </QueryClientProvider>
    )
    expect(await screen.findByText('Could not load the server settings')).toBeInTheDocument()
  })
})
