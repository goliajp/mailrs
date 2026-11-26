import type { ReactNode } from 'react'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen } from '@testing-library/react'
import { getDefaultStore } from 'jotai'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { authAtom } from '@/store/auth'

import { GroupsPanel } from '../groups-panel'
import { PasswordCell } from '../password-cell'
import { QuotaCell } from '../quota-cell'
import { quotaLabel } from '../quota-label'

const ACCOUNT = {
  active: true,
  address: 'qa@golia.jp',
  created_at: 1772571432,
  display_name: 'QA',
  domain: 'golia.jp',
  quota_bytes: 10737418240,
  recovery_email: '',
}

const PROFILE = {
  accessible_domains: [],
  address: 'lihao@golia.jp',
  display_name: 'Li Hao',
  is_admin: true,
  is_super: true,
  permissions: ['*'],
}

function renderIn(ui: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { gcTime: 0, retry: false } } })
  return render(<QueryClientProvider client={client}>{ui}</QueryClientProvider>)
}

function statusOf(body: unknown): number {
  if (body === undefined) return 404
  return 200
}

/** `null` answers 204 with no body, as the admin writes do. */
function stub(routes: Record<string, unknown>) {
  const calls: { body: unknown; method: string; path: string }[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      const method = init?.method ?? 'GET'
      calls.push({ body: init?.body && JSON.parse(String(init.body)), method, path })
      const body = routes[`${method} ${path}`]
      if (body === null) return new Response(null, { status: 204 })
      return new Response(JSON.stringify(body), {
        headers: { 'content-type': 'application/json' },
        status: statusOf(body),
      })
    })
  )
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('admin account cells', () => {
  it('changes only the password, on its own route', async () => {
    const calls = stub({ 'POST /api/admin/accounts/qa%40golia.jp/password': null })
    renderIn(<PasswordCell account={ACCOUNT} />)
    fireEvent.click(screen.getByRole('button', { name: 'Change' }))
    fireEvent.change(screen.getByLabelText('New password'), { target: { value: ' s3cret ' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await vi.waitFor(() => expect(calls).toHaveLength(1))
    expect(calls[0]).toEqual({
      body: { password: ' s3cret ' },
      method: 'POST',
      path: '/api/admin/accounts/qa%40golia.jp/password',
    })
  })

  it('offers an issuer a reset code on the row of a cleared account', async () => {
    getDefaultStore().set(authAtom, { ...PROFILE, is_reset_issuer: true })
    const calls = stub({
      'GET /api/reset-codes/qa%40golia.jp': { issued: false },
      'POST /api/reset-codes/qa%40golia.jp': { code: 'ABCDE23456', expires_at: 1790000000 },
    })
    renderIn(<PasswordCell account={{ ...ACCOUNT, password_cleared_at: 1780000000 }} />)
    expect(screen.getByText('Password cleared')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Issue a code' }))
    const inDialog = await screen.findAllByRole('button', { name: 'Issue a code' })
    fireEvent.click(inDialog[inDialog.length - 1])
    expect(await screen.findByTestId('reset-code')).toHaveTextContent('ABCDE-23456')
    expect(calls.some((c) => c.method === 'POST')).toBe(true)
  })

  it('shows the clearing without the button to someone who cannot issue', () => {
    getDefaultStore().set(authAtom, { ...PROFILE, is_reset_issuer: false })
    renderIn(<PasswordCell account={{ ...ACCOUNT, password_cleared_at: 1780000000 }} />)
    expect(screen.getByText('Password cleared')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Issue a code' })).toBeNull()
  })

  it('says nothing about clearing on an account that has its password', () => {
    getDefaultStore().set(authAtom, { ...PROFILE, is_reset_issuer: true })
    renderIn(<PasswordCell account={ACCOUNT} />)
    expect(screen.queryByText('Password cleared')).toBeNull()
    expect(screen.queryByRole('button', { name: 'Issue a code' })).toBeNull()
  })

  it('shows a stored quota and a dash for an unknown one', async () => {
    stub({ 'GET /api/admin/accounts/qa%40golia.jp/quota': { quota_bytes: 10737418240 } })
    renderIn(<QuotaCell address="qa@golia.jp" />)
    expect(await screen.findByText('10.00 GB')).toBeInTheDocument()

    expect(quotaLabel(null, 'Unlimited')).toBe('—')
    expect(quotaLabel(undefined, 'Unlimited')).toBe('—')
    expect(quotaLabel(-1, 'Unlimited')).toBe('Unlimited')
    expect(quotaLabel(0, 'Unlimited')).toBe('0 B')
  })

  it('shows a dash when the account has no quota to read', async () => {
    stub({})
    renderIn(<QuotaCell address="gone@golia.jp" />)
    expect(await screen.findByText('—')).toBeInTheDocument()
  })

  it('lists group names from a {groups} response', async () => {
    stub({ 'GET /api/admin/accounts/qa%40golia.jp/groups': { groups: ['ops'] } })
    renderIn(<GroupsPanel address="qa@golia.jp" />)
    expect(await screen.findByText('ops')).toBeInTheDocument()
  })
})
