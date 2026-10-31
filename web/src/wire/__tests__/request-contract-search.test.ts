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

  it('saved-search-create', async () => {
    const { wireCreateSavedSearch } = await import('../endpoints/saved-searches')
    const body = await bodyOf(() => wireCreateSavedSearch('Invoices', 'has:attachment invoice'))
    expect(body).toEqual(fixture('saved-search-create'))
  })
})
