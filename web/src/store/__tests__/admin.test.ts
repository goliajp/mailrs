import { describe, expect, it } from 'vitest'
import { createStore } from 'jotai/vanilla'

import { accountsAtom, aliasesAtom, domainsAtom, queueAtom } from '../admin'
import type { AccountInfo, AliasInfo, DomainInfo, QueueEntry } from '@/lib/types'

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
      { name: 'example.com', created_at: 1000 },
      { name: 'test.org', created_at: 2000 },
    ]
    store.set(domainsAtom, domains)
    expect(store.get(domainsAtom)).toEqual(domains)
  })

  it('accountsAtom can hold account info', () => {
    const store = createStore()
    const accounts: AccountInfo[] = [
      {
        address: 'alice@example.com',
        domain: 'example.com',
        display_name: 'Alice',
        active: true,
        created_at: 1000,
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
        id: 1,
        source_address: 'info@example.com',
        target_address: 'alice@example.com',
        domain: 'example.com',
        alias_type: 'forward',
        active: true,
        created_at: 1000,
      },
    ]
    store.set(aliasesAtom, aliases)
    expect(store.get(aliasesAtom)).toEqual(aliases)
  })

  it('queueAtom can hold queue entries', () => {
    const store = createStore()
    const entries: QueueEntry[] = [
      {
        id: 1,
        sender: 'alice@example.com',
        recipient: 'bob@test.org',
        domain: 'test.org',
        status: 'pending',
        attempts: 0,
        last_error: null,
        created_at: 1000,
        updated_at: 1000,
      },
    ]
    store.set(queueAtom, entries)
    expect(store.get(queueAtom)).toEqual(entries)
  })

  it('setting new value does not mutate previous value', () => {
    const store = createStore()
    const first: DomainInfo[] = [{ name: 'a.com', created_at: 1 }]
    store.set(domainsAtom, first)
    const second: DomainInfo[] = [
      { name: 'a.com', created_at: 1 },
      { name: 'b.com', created_at: 2 },
    ]
    store.set(domainsAtom, second)
    expect(first).toHaveLength(1)
    expect(store.get(domainsAtom)).toHaveLength(2)
  })

  it('each store instance is isolated', () => {
    const storeA = createStore()
    const storeB = createStore()
    storeA.set(domainsAtom, [{ name: 'a.com', created_at: 1 }])
    expect(storeB.get(domainsAtom)).toEqual([])
  })
})
