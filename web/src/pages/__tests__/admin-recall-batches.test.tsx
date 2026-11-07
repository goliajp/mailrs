// The batches re-judging took back into Review, and the super
// administrator's three decisions on them. Driven through the real wire
// client and `http` with `fetch` stubbed, so the password confirmation the
// server asks for goes through the same asker the app mounts.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { createStore, getDefaultStore, Provider } from 'jotai'
import { MemoryRouter } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const toastError = vi.fn()
const toastSuccess = vi.fn()
vi.mock('@goliapkg/gds', async () => {
  const real = await vi.importActual<Record<string, unknown>>('@goliapkg/gds')
  return {
    ...real,
    toast: { error: (m: string) => toastError(m), success: (m: string) => toastSuccess(m) },
  }
})

import { AdminSidebar } from '@/components/admin-sidebar'
import { chosenLangAtom } from '@/i18n'
import { registerConfirmAsker } from '@/lib/confirm-asker'
import { AdminRecallBatches } from '@/pages/admin-recall-batches'
import { authAtom } from '@/store/auth'

const LIST = '/api/admin/rejudge/batches'

const batch = {
  at: 1_793_700_000,
  conversations: 0,
  discarded_at: 0,
  pending: false,
  released_at: 0,
  rules: [] as string[],
  would_hold: 0,
}

const ITEMS = [
  { ...batch, observation: 'r40', pending: true, rules: ['brand-lookalike'], would_hold: 80 },
  { ...batch, conversations: 6, observation: 'r39', rules: ['payee-change'], would_hold: 6 },
  { ...batch, conversations: 3, observation: 'r38', released_at: 1_793_750_000 },
  { ...batch, discarded_at: 1_793_760_000, observation: 'r37', would_hold: 90 },
]

type Answer = { body: unknown; status: number }

function listReads(calls: string[]): number {
  return calls.filter((c) => c === `GET ${LIST}`).length
}

function row(observation: string): HTMLElement {
  return screen.getByText(observation).closest('tr') as HTMLElement
}

/** A fake server: the list, and for each write the answers in order. */
function serve(writes: Record<string, Answer[]>) {
  const calls: string[] = []
  const mock = vi.fn(async (input: string, init?: RequestInit) => {
    const method = (init?.method ?? 'GET').toUpperCase()
    calls.push(`${method} ${input}`)
    if (method === 'GET' && input === LIST) return Response.json({ items: ITEMS })
    const next = writes[input]?.shift()
    if (!next) return Response.json({ error: 'unexpected' }, { status: 500 })
    return Response.json(next.body, { status: next.status })
  })
  vi.stubGlobal('fetch', mock)
  return calls
}

function show() {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <AdminRecallBatches />
    </QueryClientProvider>
  )
}

const refused = { body: { error: 'confirm_required' }, status: 403 }

beforeEach(() => {
  getDefaultStore().set(chosenLangAtom, 'zh')
})

afterEach(() => {
  getDefaultStore().set(chosenLangAtom, null)
  registerConfirmAsker(null)
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

describe('recall batches', () => {
  it('shows each batch with its state, count, rules and time', async () => {
    serve({})
    show()
    expect(await screen.findByText('r40')).toBeDefined()
    expect(within(row('r40')).getByText('等待确认')).toBeDefined()
    expect(within(row('r40')).getByText('80')).toBeDefined()
    expect(within(row('r40')).getByText('brand-lookalike')).toBeDefined()
    expect(within(row('r39')).getByText('已撤回')).toBeDefined()
    expect(within(row('r39')).getByText('6')).toBeDefined()
    expect(within(row('r38')).getByText('已放回')).toBeDefined()
    expect(within(row('r37')).getByText('已放弃')).toBeDefined()
  })

  it('offers confirm and discard on a waiting batch, release on a held one, nothing on the rest', async () => {
    serve({})
    show()
    await screen.findByText('r40')
    expect(within(row('r40')).getByRole('button', { name: '确认撤回' })).toBeDefined()
    expect(within(row('r40')).getByRole('button', { name: '放弃' })).toBeDefined()
    expect(within(row('r39')).getByRole('button', { name: '整批放回' })).toBeDefined()
    expect(within(row('r38')).queryAllByRole('button')).toEqual([])
    expect(within(row('r37')).queryAllByRole('button')).toEqual([])
  })

  const writes = [
    { button: '确认撤回', done: { held: 78 }, observation: 'r40', verb: 'confirm' },
    { button: '放弃', done: { would_hold: 80 }, observation: 'r40', verb: 'discard' },
    { button: '整批放回', done: { released: 6 }, observation: 'r39', verb: 'release' },
  ]

  for (const w of writes) {
    it(`asks for the password before ${w.verb}, then sends it again and refreshes`, async () => {
      const path = `${LIST}/${w.observation}/${w.verb}`
      const calls = serve({ [path]: [refused, { body: w.done, status: 200 }] })
      const asker = vi.fn(async () => true)
      registerConfirmAsker(asker)
      show()
      await screen.findByText(w.observation)
      fireEvent.click(within(row(w.observation)).getByRole('button', { name: w.button }))
      await waitFor(() => expect(listReads(calls)).toBe(2))
      expect(asker).toHaveBeenCalledTimes(1)
      expect(calls.filter((c) => c === `POST ${path}`)).toHaveLength(2)
      expect(toastSuccess).toHaveBeenCalledTimes(1)
      expect(toastError).not.toHaveBeenCalled()
    })

    it(`says the batch was already handled when ${w.verb} answers 409, and refreshes`, async () => {
      const path = `${LIST}/${w.observation}/${w.verb}`
      const calls = serve({ [path]: [{ body: {}, status: 409 }] })
      show()
      await screen.findByText(w.observation)
      fireEvent.click(within(row(w.observation)).getByRole('button', { name: w.button }))
      await waitFor(() => expect(listReads(calls)).toBe(2))
      expect(toastError).toHaveBeenCalledWith('这个批次已经处理过')
    })
  }

  it('does nothing when the password is not given', async () => {
    const path = `${LIST}/r39/release`
    const calls = serve({ [path]: [refused] })
    registerConfirmAsker(async () => false)
    show()
    await screen.findByText('r39')
    fireEvent.click(within(row('r39')).getByRole('button', { name: '整批放回' }))
    await waitFor(() => expect(toastError).toHaveBeenCalledWith('没有确认密码，什么都没有做'))
    expect(calls.filter((c) => c === `POST ${path}`)).toHaveLength(1)
  })
})

describe('who sees the recall batches page', () => {
  function sidebar(isSuper: boolean) {
    const store = createStore()
    store.set(chosenLangAtom, 'zh')
    store.set(authAtom, {
      accessible_domains: [],
      address: 'admin@example.com',
      display_name: 'A',
      is_admin: true,
      is_super: isSuper,
      permissions: ['admin.accounts', 'admin.domains', 'admin.system_config'],
    })
    render(
      <Provider store={store}>
        <MemoryRouter>
          <AdminSidebar />
        </MemoryRouter>
      </Provider>
    )
  }

  it('is in the admin navigation for a super administrator', () => {
    sidebar(true)
    expect(screen.getAllByText('撤回批次').length).toBeGreaterThan(0)
  })

  it('is not there for an administrator who is not super', () => {
    sidebar(false)
    expect(screen.getAllByText('账号').length).toBeGreaterThan(0)
    expect(screen.queryAllByText('撤回批次')).toEqual([])
  })
})
