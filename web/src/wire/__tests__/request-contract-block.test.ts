// Blocking a sender: the same contract as `request-contract.test.ts`; the
// Rust side is `crates/webapi/tests/request_contract_block.rs`.

import { describe, expect, it } from 'vitest'

import { bodyOf, capturedRequest, fixture } from './request-contract-harness'

describe('block sender request bodies match the shared contract', () => {
  it('block sender', async () => {
    const { wireBlockSender } = await import('../endpoints/block-sender')
    const body = await bodyOf(() => wireBlockSender('t1', 'friend@example.com'))
    expect(body).toEqual(fixture('block-sender'))
    expect(capturedRequest().path).toContain('/conversations/t1/block-sender')
  })

  it('unblock sender, the undo', async () => {
    const { wireUnblockSender } = await import('../endpoints/block-sender')
    const body = await bodyOf(() => wireUnblockSender('t1', 'friend@example.com', true))
    expect(body).toEqual(fixture('unblock-sender'))
    expect(capturedRequest().path).toContain('/conversations/t1/unblock-sender')
  })
})
