import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AdminGreylist } from '../admin-greylist'

type Call = { body: unknown; method: string; path: string }

/** `list_greylist_local` rows: a current one and one the not-junk path wrote under its own key. */
const ENTRIES = {
  items: [
    {
      created_at: 1_792_206_288,
      created_by: 'lihao@golia.jp',
      id: 7,
      kind: 'cidr',
      list: 'black',
      note: 'spam run',
      value: '203.0.113.0/24',
    },
    {
      created_at: 5,
      created_by: null,
      id: 'not-junk:a@b.c',
      kind: 'email',
      list: 'white',
      note: null,
      value: 'a@b.c',
    },
  ],
}

function renderAt(url = '/') {
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[url]}>
        <AdminGreylist />
      </MemoryRouter>
    </QueryClientProvider>
  )
}

function stub(routes: Record<string, unknown>) {
  const calls: Call[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      const method = init?.method ?? 'GET'
      calls.push({ body: init?.body && JSON.parse(String(init.body)), method, path })
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

describe('<AdminGreylist />', () => {
  it('deletes a not-junk entry by its stored key after saying what changes', async () => {
    const calls = stub({
      'DELETE /api/admin/greylist/local-lists/not-junk%3Aa%40b.c': null,
      'GET /api/admin/greylist/local-lists': ENTRIES,
    })
    renderAt()
    const row = (await screen.findByText('a@b.c')).closest('tr') as HTMLElement
    fireEvent.click(within(row).getByRole('button', { name: 'Delete' }))
    const dialog = await screen.findByRole('alertdialog', {
      name: 'Delete a@b.c from the white list?',
    })
    expect(
      within(dialog).getByText(
        "Mail from a@b.c no longer goes to the inbox through this entry and is checked for spam like anyone else's. The entry is deleted, and the receiver drops it at its next reload, within a minute"
      )
    ).toBeInTheDocument()
    expect(calls.some((c) => c.method === 'DELETE')).toBe(false)
    fireEvent.click(within(dialog).getByRole('button', { name: 'Delete entry' }))
    await vi.waitFor(() =>
      expect(
        calls.some(
          (c) =>
            c.method === 'DELETE' && c.path === '/api/admin/greylist/local-lists/not-junk%3Aa%40b.c'
        )
      ).toBe(true)
    )
  })

  it('says a black entry stops refusing mail once deleted', async () => {
    stub({ 'GET /api/admin/greylist/local-lists': ENTRIES })
    renderAt()
    const row = (await screen.findByText('203.0.113.0/24')).closest('tr') as HTMLElement
    fireEvent.click(within(row).getByRole('button', { name: 'Delete' }))
    expect(
      await screen.findByText(/^Mail from 203\.0\.113\.0\/24 is no longer refused by this entry/)
    ).toBeInTheDocument()
  })

  it('says the lists apply without greylisting and white needs proof', async () => {
    stub({ 'GET /api/admin/greylist/local-lists': { items: [] } })
    renderAt()
    expect(await screen.findByText('No sender list entries yet')).toBeInTheDocument()
    expect(screen.getAllByText(/apply whether or not greylisting is on/).length).toBeGreaterThan(0)
    expect(screen.getAllByText(/counts only when SPF or DKIM proves/).length).toBeGreaterThan(0)
    expect(screen.queryByText(/only while greylisting is on/)).toBeNull()
  })

  it('filters by the list in the URL', async () => {
    stub({ 'GET /api/admin/greylist/local-lists': ENTRIES })
    renderAt('/?list=black')
    expect(await screen.findByText('203.0.113.0/24')).toBeInTheDocument()
    expect(screen.queryByText('a@b.c')).toBeNull()
  })

  it('adds an entry with the same body as before', async () => {
    const calls = stub({
      'GET /api/admin/greylist/local-lists': ENTRIES,
      'POST /api/admin/greylist/local-lists': { id: 8 },
    })
    renderAt()
    await screen.findByText('a@b.c')
    fireEvent.click(screen.getByRole('button', { name: 'Add entry' }))
    fireEvent.change(screen.getByLabelText('Value'), { target: { value: ' example.org ' } })
    fireEvent.click(screen.getByRole('button', { name: 'Add entry' }))
    await vi.waitFor(() => expect(calls.some((c) => c.method === 'POST')).toBe(true))
    expect(calls.find((c) => c.method === 'POST')?.body).toEqual({
      kind: 'domain',
      list: 'white',
      note: null,
      value: 'example.org',
    })
  })
})
