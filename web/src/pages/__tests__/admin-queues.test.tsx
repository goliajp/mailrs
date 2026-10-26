import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AdminQueues } from '../admin-queues'

type Call = { method: string; path: string }

/** What `list_admin_queue` answers: stored envelopes without the body, labelled by state. */
const QUEUE = {
  items: [
    { attempts: 0, id: 1, recipient: 'a@one.example', sender: 's@b.c', status: 'pending' },
    {
      attempts: 3,
      id: 2,
      last_error: '421 try later',
      recipient: 'b@two.example',
      sender: 's@b.c',
      status: 'retrying',
    },
  ],
}

function renderAt(url = '/') {
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[url]}>
        <AdminQueues />
      </MemoryRouter>
    </QueryClientProvider>
  )
}

/** Answer `METHOD path` from `routes`; null is a 204, anything unlisted a 404. */
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

describe('<AdminQueues />', () => {
  it('offers a retry only for a job waiting between attempts', async () => {
    stub({ 'GET /api/admin/queues': QUEUE })
    renderAt()
    const retryingRow = (await screen.findByText('b@two.example')).closest('tr') as HTMLElement
    const pendingRow = screen.getByText('a@one.example').closest('tr') as HTMLElement
    expect(within(retryingRow).getByText('two.example')).toBeInTheDocument()
    expect(within(retryingRow).getByText('421 try later')).toBeInTheDocument()
    expect(within(pendingRow).queryByRole('button', { name: 'Retry now' })).toBeNull()
  })

  it('says what a retry does and sends it only once confirmed', async () => {
    const calls = stub({ 'GET /api/admin/queues': QUEUE, 'POST /api/queue/2/retry': null })
    renderAt()
    const row = (await screen.findByText('b@two.example')).closest('tr') as HTMLElement
    fireEvent.click(within(row).getByRole('button', { name: 'Retry now' }))
    const dialog = await screen.findByRole('alertdialog', { name: 'Retry job #2 now?' })
    expect(
      within(dialog).getByText(
        'The message to b@two.example stops waiting out its backoff and joins the send queue now. It still counts as an attempt: a temporary failure parks it again with a longer wait, a permanent one moves it to failed and sends s@b.c a bounce notice'
      )
    ).toBeInTheDocument()
    expect(calls.some((c) => c.method === 'POST')).toBe(false)
    fireEvent.click(within(dialog).getByRole('button', { name: 'Retry now' }))
    await vi.waitFor(() =>
      expect(calls.some((c) => c.method === 'POST' && c.path === '/api/queue/2/retry')).toBe(true)
    )
    expect(calls.some((c) => c.path === '/api/queue')).toBe(false)
  })

  it('sends nothing when the retry is cancelled', async () => {
    const calls = stub({ 'GET /api/admin/queues': QUEUE })
    renderAt()
    const row = (await screen.findByText('b@two.example')).closest('tr') as HTMLElement
    fireEvent.click(within(row).getByRole('button', { name: 'Retry now' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }))
    expect(screen.queryByRole('alertdialog')).toBeNull()
    expect(calls.some((c) => c.method === 'POST')).toBe(false)
  })

  it('filters by the status in the URL', async () => {
    stub({ 'GET /api/admin/queues': QUEUE })
    renderAt('/?status=pending')
    expect(await screen.findByText('a@one.example')).toBeInTheDocument()
    expect(screen.queryByText('b@two.example')).toBeNull()
  })

  it('lists mail that failed for good under Failed, with why and no retry', async () => {
    const failed = {
      failed_at: 1_792_871_464,
      id: 348,
      last_error: '550 5.1.1 no such user',
      recipient: '',
      sender: '',
      status: 'failed',
    }
    stub({ 'GET /api/admin/queues': { items: [...QUEUE.items, failed] } })
    renderAt('/?status=failed')
    const reason = await screen.findByText('550 5.1.1 no such user')
    const row = reason.closest('tr') as HTMLElement
    expect(screen.queryByText('a@one.example')).toBeNull()
    expect(within(row).getAllByText('—').length).toBeGreaterThanOrEqual(2)
    expect(within(row).queryByRole('button', { name: 'Retry now' })).toBeNull()
    expect(screen.getByRole('button', { name: /failed \(1\)/i })).toBeInTheDocument()
  })

  it('names what failed to load', async () => {
    stub({})
    renderAt()
    expect(await screen.findByText('Could not load the outbound queue')).toBeInTheDocument()
  })

  it('reads an empty queue as nothing waiting', async () => {
    stub({ 'GET /api/admin/queues': { items: [] } })
    renderAt()
    expect(await screen.findByText('The queue is empty')).toBeInTheDocument()
  })
})
