import { describe, expect, it } from 'vitest'

import { bodyOf, fixture } from './request-contract-harness'

describe('search request bodies match the shared contract', () => {
  it('rule-create, rule-preview', async () => {
    const { wireCreateRule } = await import('../endpoints/rules')
    const { wireUpdateRule } = await import('../endpoints/rules')
    const { wirePreviewRule } = await import('../endpoints/rules')
    const rule = {
      actions: ['archive', 'label:3'],
      enabled: true,
      name: 'Shop',
      query: 'from:shop.example has:attachment',
    }
    expect(await bodyOf(() => wireCreateRule(rule))).toEqual(fixture('rule-create'))
    expect(await bodyOf(() => wireUpdateRule('1', rule))).toEqual(fixture('rule-create'))
    expect(await bodyOf(() => wirePreviewRule('from:shop.example'))).toEqual(
      fixture('rule-preview')
    )
  })

  /**
   * Search is a GET: the fixture is its query parameters. Every list axis
   * is in it, because the list and the search inside it must scope to the
   * same thing; `request_contract_search.rs` reads the same file.
   */
  it('conversation-search', async () => {
    const { listPath } = await import('@/lib/list-axes')
    const path = listPath({
      accounts: ['', 'ext_a'],
      archived: true,
      assignedToMe: true,
      category: 'notification',
      folder: 'Inbox',
      followupDueBy: 1700000650,
      includeQuarantined: true,
      label: '3',
      pinned: true,
      query: 'invoice',
      section: 'awaiting',
      snoozed: true,
      starred: true,
      trashed: true,
      unread: true,
    })
    const url = new URL(path, 'http://mail.test')
    expect(url.pathname).toBe('/conversations/search')
    expect(Object.fromEntries(url.searchParams)).toEqual(fixture('conversation-search'))
  })

  it('a search from Review is a search of Review', async () => {
    const { listPath } = await import('@/lib/list-axes')
    const url = new URL(listPath({ quarantined: true, query: 'invoice' }), 'http://mail.test')
    expect(url.pathname).toBe('/conversations/search')
    expect(url.searchParams.get('quarantined')).toBe('true')
  })

  it('the list sends the axes its search sends', async () => {
    const { listPath } = await import('@/lib/list-axes')
    const filters = { assignedToMe: true, label: '3', pinned: true, section: 'awaiting' }
    const list = new URL(listPath(filters), 'http://mail.test').searchParams
    const search = new URL(listPath({ ...filters, query: 'x' }), 'http://mail.test').searchParams
    search.delete('q')
    expect(Object.fromEntries(search)).toEqual(Object.fromEntries(list))
    expect(list.get('assigned_to_me')).toBe('true')
  })

  it('saved-search-create', async () => {
    const { wireCreateSavedSearch } = await import('../endpoints/saved-searches')
    const body = await bodyOf(() => wireCreateSavedSearch('Invoices', 'has:attachment invoice'))
    expect(body).toEqual(fixture('saved-search-create'))
  })
})
