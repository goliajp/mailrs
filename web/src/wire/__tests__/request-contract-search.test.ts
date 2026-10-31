import { describe, expect, it } from 'vitest'

import { bodyOf, fixture } from './request-contract-harness'

describe('search request bodies match the shared contract', () => {
  it('saved-search-create', async () => {
    const { wireCreateSavedSearch } = await import('../endpoints/saved-searches')
    const body = await bodyOf(() => wireCreateSavedSearch('Invoices', 'has:attachment invoice'))
    expect(body).toEqual(fixture('saved-search-create'))
  })
})
