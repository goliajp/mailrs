import { describe, expect, it } from 'vitest'

import { bodyOf, capturedRequest, fixture } from './request-contract-harness'

describe('masked address request bodies match the shared contract', () => {
  it('masked-email-create, masked-email-update', async () => {
    const { wireCreateMaskedEmail } = await import('../endpoints/masked-email')
    const { wireSetMaskedEmailState } = await import('../endpoints/masked-email')
    const create = await bodyOf(() =>
      wireCreateMaskedEmail({ description: 'orders', forDomain: 'shop.example' })
    )
    expect(create).toEqual(fixture('masked-email-create'))
    expect(capturedRequest().method).toBe('POST')
    const update = await bodyOf(() =>
      wireSetMaskedEmailState('amber.otter.0042@golia.jp', 'disabled')
    )
    expect(update).toEqual(fixture('masked-email-update'))
    expect(capturedRequest().method).toBe('PATCH')
  })
})
