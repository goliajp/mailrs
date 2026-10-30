import { QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { queryClient } from '@/lib/query-client'

import { PausedSenders } from '../paused-senders'

const PAUSED = '/api/admin/outbound/paused'
const RESUME = '/api/admin/outbound/paused/spam%40golia.jp/resume'

function stub(routes: Record<string, unknown[]>) {
  const calls: { body: unknown; method: string; path: string }[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      const method = init?.method ?? 'GET'
      calls.push({ body: init?.body && JSON.parse(String(init.body)), method, path })
      const queue = routes[`${method} ${path}`] ?? []
      const body = queue.length > 1 ? queue.shift() : queue[0]
      return new Response(JSON.stringify(body ?? {}), {
        headers: { 'content-type': 'application/json' },
        status: 200,
      })
    })
  )
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
  queryClient.clear()
})

function renderPanel() {
  render(
    <QueryClientProvider client={queryClient}>
      <PausedSenders />
    </QueryClientProvider>
  )
}

describe('paused senders', () => {
  it('shows nothing while no account is paused', async () => {
    const calls = stub({ [`GET ${PAUSED}`]: [{ items: [] }] })
    renderPanel()
    await vi.waitFor(() => expect(calls).toHaveLength(1))
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('names the account and releases it as chosen', async () => {
    const one = {
      address: 'spam@golia.jp',
      held: 3,
      reason: '201 recipients in an hour',
      since: 1793368800,
    }
    const calls = stub({
      [`GET ${PAUSED}`]: [{ items: [one] }, { items: [] }],
      [`POST ${RESUME}`]: [{ discarded: 3, sent: 0 }],
    })
    renderPanel()
    await screen.findByText('Outbound mail from spam@golia.jp is paused')
    expect(screen.getByText(/3 messages held/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Release and discard' }))
    // the confirmation's own button, after the panel's
    const buttons = await screen.findAllByRole('button', { name: 'Release and discard' })
    fireEvent.click(buttons[buttons.length - 1])
    await screen.findByText(/0 sent, 3 discarded/)
    expect(calls.find((c) => c.method === 'POST')).toEqual({
      body: { held: 'discard' },
      method: 'POST',
      path: RESUME,
    })
  })
})
