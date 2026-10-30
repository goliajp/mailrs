import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { CompromiseResetDialog } from '../compromise-reset-dialog'

const KEYS = '/api/admin/accounts/qa%40golia.jp/agent-keys'
const RESET = '/api/admin/accounts/qa%40golia.jp/password'

const used = {
  created_at: 1772571432,
  id: 1,
  last_used_at: 1793368800,
  name: 'insight',
  prefix: 'mk_1a2b3',
  scopes: ['mail:send'],
}
const idle = { ...used, id: 2, last_used_at: null, name: 'old', prefix: 'mk_9f8e7' }

function renderDialog() {
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <CompromiseResetDialog address="qa@golia.jp" onClose={() => {}} />
    </QueryClientProvider>
  )
}

/** Each route answers its queued replies in turn: `[status, body]`. */
function stub(routes: Record<string, [number, unknown][]>) {
  const calls: { body: unknown; method: string; path: string }[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      const method = init?.method ?? 'GET'
      calls.push({ body: init?.body && JSON.parse(String(init.body)), method, path })
      const [status, body] = routes[`${method} ${path}`]?.shift() ?? [404, {}]
      return new Response(JSON.stringify(body), {
        headers: { 'content-type': 'application/json' },
        status,
      })
    })
  )
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('compromise reset dialog', () => {
  it('lists the keys and says when a key has no recorded use', async () => {
    stub({
      [`GET ${KEYS}`]: [[200, { items: [used, idle], last_use_recorded_since: '2026-10-31' }]],
    })
    renderDialog()
    await screen.findByText('insight')
    expect(screen.getByText('Not recorded (recording began 2026-10-31)')).toBeTruthy()
    expect(screen.getAllByText(/to the hour/)).toHaveLength(1)
    expect(screen.queryByText(/never/i)).toBeNull()
    expect(screen.queryByText(/1970/)).toBeNull()
  })

  it('names every key shown, and shows the new list when it changed', async () => {
    const calls = stub({
      [`GET ${KEYS}`]: [[200, { items: [used], last_use_recorded_since: '2026-10-31' }]],
      [`POST ${RESET}`]: [
        [409, { error: 'key_list_changed', keys: [used, idle] }],
        [200, { agent_keys_revoked: 2, sessions_revoked: 1 }],
      ],
    })
    renderDialog()
    await screen.findByText('insight')
    fireEvent.change(screen.getByLabelText('New password'), {
      target: { value: 'a much longer new passphrase' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Reset and revoke 1 keys' }))
    await screen.findByText('old')
    expect(screen.getByRole('status').textContent).toMatch(/keys changed/)
    expect(calls.filter((c) => c.method === 'GET')).toHaveLength(1)

    fireEvent.click(screen.getByRole('button', { name: 'Reset and revoke 2 keys' }))
    await screen.findByText(/2 agent keys revoked/)
    const posts = calls.filter((c) => c.method === 'POST')
    expect(posts.map((p) => p.body)).toEqual([
      { compromised: { revoke_key_ids: [1] }, password: 'a much longer new passphrase' },
      { compromised: { revoke_key_ids: [1, 2] }, password: 'a much longer new passphrase' },
    ])
  })
})
