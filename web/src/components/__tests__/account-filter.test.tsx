import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { createStore, Provider } from 'jotai'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { canonicaliseFilter } from '@/domain'
import { selectedAccountsAtom } from '@/store/ui'

const listed = vi.fn()
vi.mock('@/wire/endpoints/external-accounts', () => ({
  wireListExternalAccounts: () => listed(),
}))

const { AccountFilter } = await import('../account-filter')

function account(id: string, name: string) {
  return {
    auth: 'app_password',
    colour: '#22c55e',
    created_at: 0,
    display_name: name,
    email: `${name}@x.com`,
    failures: 0,
    id,
    incoming: { host: 'h', port: 993, protocol: 'imap', tls: 'implicit' },
    last_error: null,
    last_sync: 0,
    next_attempt: 0,
    outgoing: { host: 'h', port: 587, protocol: 'smtp', tls: 'starttls' },
    provider: 'custom',
    sort: 0,
    state: 'ok',
    username: null,
  }
}

function show(store = createStore()) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const view = render(
    <QueryClientProvider client={client}>
      <Provider store={store}>
        <AccountFilter />
      </Provider>
    </QueryClientProvider>
  )
  return { store, view }
}

beforeEach(() => {
  vi.clearAllMocks()
  listed.mockResolvedValue([account('ext_a', 'Work'), account('ext_b', 'Uni')])
})

describe('narrowing the list to some accounts', () => {
  /** A filter over one mailbox is furniture. */
  it('is not rendered when there is nothing to leave out', async () => {
    listed.mockResolvedValue([])
    const { view } = show()
    await new Promise((r) => setTimeout(r, 0))
    expect(view.container.textContent).toBe('')
  })

  it('starts with every account on', async () => {
    show()
    expect(await screen.findByRole('button', { name: /all accounts/i })).toBeTruthy()
  })

  /**
   * "Only these", not "only this": unticking one leaves the others on.
   */
  it('unticking one narrows to the rest', async () => {
    const { store } = show()
    await userEvent.click(await screen.findByRole('button', { name: /accounts/i }))
    await userEvent.click(await screen.findByText('Uni'))
    expect(store.get(selectedAccountsAtom)).toEqual(['', 'ext_a'])
  })

  /** Back to everything is no filter, not a filter naming everything. */
  it('re-ticking the last one clears the filter', async () => {
    const store = createStore()
    store.set(selectedAccountsAtom, ['', 'ext_a'])
    show(store)
    await userEvent.click(await screen.findByRole('button', { name: /accounts/i }))
    await userEvent.click(await screen.findByText('Uni'))
    expect(store.get(selectedAccountsAtom)).toBeNull()
  })

  /** This server's own mail is an account like the rest. */
  it('offers this server as something to switch off', async () => {
    const { store } = show()
    await userEvent.click(await screen.findByRole('button', { name: /accounts/i }))
    await userEvent.click(await screen.findByText('This server'))
    expect(store.get(selectedAccountsAtom)).toEqual(['ext_a', 'ext_b'])
  })
})

describe('the query key', () => {
  /**
   * The one that would serve the wrong list from cache: unticking every
   * account and being shown all of it, because `[]` keyed the same as
   * "no filter".
   */
  it('tells no filter apart from a filter that selects nothing', () => {
    const none = canonicaliseFilter({ accounts: [] })
    const every = canonicaliseFilter({})
    expect(none.accounts).not.toBe(every.accounts)
    expect(every.accounts).toBeNull()
  })

  it('does not care what order the accounts were ticked in', () => {
    expect(canonicaliseFilter({ accounts: ['b', 'a'] }).accounts).toBe(
      canonicaliseFilter({ accounts: ['a', 'b'] }).accounts
    )
  })
})
