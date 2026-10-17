import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AdminSystemConfig } from '../admin-system-config'

/** Two rows of the prod capture plus one saved override, as `get_system_config` builds it. */
const CONFIG = {
  entries: [
    {
      default_value: '25',
      description: 'Inbound SMTP port on the receiver process',
      group: 'smtp',
      key: 'smtp_port',
      source: 'default',
      value: '25',
      value_type: 'string',
    },
    {
      default_value: '',
      description: 'Public SMTP hostname (HELO / greeting)',
      group: 'smtp',
      key: 'hostname',
      source: 'database',
      updated_at: 1_792_206_288,
      updated_by: 'lihao@golia.jp',
      value: 'mail.golia.ai',
      value_type: 'string',
    },
    {
      default_value: '143',
      description: 'IMAP port on the fastcore process',
      group: 'imap',
      key: 'imap_port',
      source: 'env',
      value: '143',
      value_type: 'string',
    },
  ],
  success: true,
}

function renderPage() {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () =>
      Promise.resolve(
        new Response(JSON.stringify(CONFIG), {
          headers: { 'content-type': 'application/json' },
          status: 200,
        })
      )
    )
  )
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <AdminSystemConfig />
      </MemoryRouter>
    </QueryClientProvider>
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('<AdminSystemConfig />', () => {
  it('names the groups the backend sends and who saved an override', async () => {
    renderPage()
    expect(await screen.findByText('SMTP')).toBeInTheDocument()
    expect(screen.getByText('IMAP')).toBeInTheDocument()
    expect(screen.getByText(/^Saved by lihao@golia\.jp on /)).toBeInTheDocument()
    expect(screen.getAllByText(/^Saved/)).toHaveLength(1)
    expect(screen.getByLabelText('hostname')).toHaveValue('mail.golia.ai')
  })

  it('does not claim a save takes effect', async () => {
    renderPage()
    await screen.findByText('SMTP')
    expect(screen.queryByText(/immediately/)).toBeNull()
  })
})
