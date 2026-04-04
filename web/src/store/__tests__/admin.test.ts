import type { AccountInfo, AliasInfo, DomainInfo, QueueEntry } from '@/lib/types'

import { createStore } from 'jotai/vanilla'
import { describe, expect, it } from 'vitest'

import { accountsAtom, aliasesAtom, domainsAtom, queueAtom } from '../admin'

describe('admin atoms — initial values', () => {
  it('domainsAtom defaults to empty array', () => {
    const store = createStore()
    expect(store.get(domainsAtom)).toEqual([])
  })

  it('accountsAtom defaults to empty array', () => {
    const store = createStore()
    expect(store.get(accountsAtom)).toEqual([])
  })

  it('aliasesAtom defaults to empty array', () => {
    const store = createStore()
    expect(store.get(aliasesAtom)).toEqual([])
  })

  it('queueAtom defaults to empty array', () => {
    const store = createStore()
    expect(store.get(queueAtom)).toEqual([])
  })
})

describe('admin atoms — writability', () => {
  it('domainsAtom can hold domain info', () => {
    const store = createStore()
    const domains: DomainInfo[] = [
      { created_at: 1000, name: 'example.com' },
      { created_at: 2000, name: 'test.org' },
    ]
    store.set(domainsAtom, domains)
    expect(store.get(domainsAtom)).toEqual(domains)
  })

  it('accountsAtom can hold account info', () => {
    const store = createStore()
    const accounts: AccountInfo[] = [
      {
        active: true,
        address: 'alice@example.com',
        created_at: 1000,
        display_name: 'Alice',
        domain: 'example.com',
        quota_bytes: 0,
        recovery_email: '',
      },
    ]
    store.set(accountsAtom, accounts)
    expect(store.get(accountsAtom)).toEqual(accounts)
  })

  it('aliasesAtom can hold alias info', () => {
    const store = createStore()
    const aliases: AliasInfo[] = [
      {
        active: true,
        alias_type: 'forward',
        created_at: 1000,
        domain: 'example.com',
        id: 1,
        source_address: 'info@example.com',
        target_address: 'alice@example.com',
      },
    ]
    store.set(aliasesAtom, aliases)
    expect(store.get(aliasesAtom)).toEqual(aliases)
  })

  it('queueAtom can hold queue entries', () => {
    const store = createStore()
    const entries: QueueEntry[] = [
      {
        attempts: 0,
        created_at: 1000,
        domain: 'test.org',
        id: 1,
        last_error: null,
        recipient: 'bob@test.org',
        sender: 'alice@example.com',
        status: 'pending',
        updated_at: 1000,
      },
    ]
    store.set(queueAtom, entries)
    expect(store.get(queueAtom)).toEqual(entries)
  })

  it('setting new value does not mutate previous value', () => {
    const store = createStore()
    const first: DomainInfo[] = [{ created_at: 1, name: 'a.com' }]
    store.set(domainsAtom, first)
    const second: DomainInfo[] = [
      { created_at: 1, name: 'a.com' },
      { created_at: 2, name: 'b.com' },
    ]
    store.set(domainsAtom, second)
    expect(first).toHaveLength(1)
    expect(store.get(domainsAtom)).toHaveLength(2)
  })

  it('each store instance is isolated', () => {
    const storeA = createStore()
    const storeB = createStore()
    storeA.set(domainsAtom, [{ created_at: 1, name: 'a.com' }])
    expect(storeB.get(domainsAtom)).toEqual([])
  })
})
