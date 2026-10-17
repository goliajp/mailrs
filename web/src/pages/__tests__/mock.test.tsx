import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { createStore, Provider as JotaiProvider } from 'jotai'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { authAtom } from '@/store/auth'

import { MockMail } from '../mock'

const NAMESPACE = {
  address: 'insight.test@mock.golia.jp',
  bytes: 0,
  created_at: 1_792_195_000_000,
  dropped: 0,
  messages: 2,
  name: 'insight',
  owner: 'lihao@golia.jp',
  smtp_enabled: false,
  webhook_url: null,
}

const META = {
  attachments: [],
  cc: [],
  codes: [],
  from: 'GitHub <noreply@github.com>',
  links: [],
  message_id: null,
  namespace: 'insight',
  rcpt: 'insight.signup@mock.golia.jp',
  received_at: 1_792_195_200_123,
  size: 100,
  tag: 'signup',
  to: ['insight.signup@mock.golia.jp'],
  uid: 1,
}

function renderAt(url: string, permissions: string[] = []) {
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  const store = createStore()
  store.set(authAtom, {
    accessible_domains: [],
    address: 'lihao@golia.jp',
    display_name: 'L',
    permissions,
    token: 't',
  })
  return render(
    <JotaiProvider store={store}>
      <QueryClientProvider client={client}>
        <MemoryRouter initialEntries={[url]}>
          <MockMail />
        </MemoryRouter>
      </QueryClientProvider>
    </JotaiProvider>
  )
}

/** Answer each path with `[status, body]`; anything unlisted is a 500 so a stray call is loud. */
function stubFetch(routes: Record<string, [number, unknown]>) {
  const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
    const path = String(input).replace(/\?.*$/, '')
    const hit = routes[path]
    if (hit === undefined) return new Response('{"error":"unexpected"}', { status: 500 })
    return new Response(JSON.stringify(hit[1]), {
      headers: { 'content-type': 'application/json' },
      status: hit[0],
    })
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('<MockMail />', () => {
  it('says the feature is off when the namespace list answers 404', async () => {
    const fetchMock = stubFetch({
      '/api/mock/namespaces': [404, { error: 'mock mail is not enabled on this deployment' }],
    })
    renderAt('/mock')
    expect(
      await screen.findByText('Mock mail is not enabled on this deployment')
    ).toBeInTheDocument()
    // a settled answer, not retried
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('region', { name: 'Namespaces' })).toBeNull()
  })

  it('shows a server error as an error, not as the feature being off', async () => {
    stubFetch({ '/api/mock/namespaces': [500, { error: 'kevy unavailable' }] })
    renderAt('/mock')
    // the page retries anything but a 404 once, about a second later
    expect(
      await screen.findByText('kevy unavailable', undefined, { timeout: 3000 })
    ).toBeInTheDocument()
    expect(screen.queryByText('Mock mail is not enabled on this deployment')).toBeNull()
  })

  it('lists the namespace from the URL and its mail with direction badges', async () => {
    stubFetch({
      '/api/mock/namespaces': [200, { namespaces: [NAMESPACE] }],
      '/api/mock/namespaces/insight/messages': [
        200,
        {
          dropped: 0,
          messages: [
            { ...META, direction: 'captured-out', id: 'm2', subject: 'Welcome aboard' },
            { ...META, direction: 'in', id: 'm1', subject: 'Your code' },
          ],
        },
      ],
    })
    renderAt('/mock?ns=insight')
    expect(await screen.findByText('Welcome aboard')).toBeInTheDocument()
    expect(screen.getByText('Your code')).toBeInTheDocument()
    expect(screen.getByText('captured-out')).toBeInTheDocument()
    expect(screen.getByText('in')).toBeInTheDocument()
    expect(screen.getByText('insight.test@mock.golia.jp')).toBeInTheDocument()
    expect(screen.getByText('2 messages, 0 B, 0 dropped')).toBeInTheDocument()
  })

  it('lets an admin list everyone’s namespaces, with their owners', async () => {
    const fetchMock = stubFetch({
      '/api/mock/namespaces': [200, { namespaces: [{ ...NAMESPACE, owner: 'ann@golia.jp' }] }],
    })
    renderAt('/mock?scope=all', ['admin.accounts'])
    expect(await screen.findByText('ann@golia.jp')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Everyone’s' })).toHaveAttribute(
      'aria-pressed',
      'true'
    )
    expect(String(fetchMock.mock.calls[0][0])).toContain('all=true')
    expect(screen.queryByLabelText('New namespace name')).toBeNull()
  })

  it('ignores the scope for someone who is not an admin', async () => {
    const fetchMock = stubFetch({ '/api/mock/namespaces': [200, { namespaces: [NAMESPACE] }] })
    renderAt('/mock?scope=all')
    expect(await screen.findByText('insight')).toBeInTheDocument()
    expect(String(fetchMock.mock.calls[0][0])).not.toContain('all=true')
    expect(screen.queryByRole('group', { name: 'Whose namespaces' })).toBeNull()
  })
})
