// The ops daemon's silence and maintenance bodies: the same contract as
// `request-contract.test.ts`; the Rust side is `ops_bodies_match` in
// `crates/webapi/tests/request_contract_admin.rs`.

import { describe, expect, it } from 'vitest'

import { bodyOf, capturedRequest, fixture } from './request-contract-harness'

describe('ops request bodies match the shared contract', () => {
  it('set a silence', async () => {
    const { wireOpsSilence } = await import('../endpoints/ops')
    const body = await bodyOf(() =>
      wireOpsSilence({
        forSecs: 3600,
        object: 'spool',
        reason: 'known file',
        rule: 'spool_quarantined',
      })
    )
    expect(body).toEqual(fixture('ops-silence-set'))
    expect(capturedRequest().path).toContain('/admin/ops/silences')
  })

  it('end a silence', async () => {
    const { wireOpsEndSilence } = await import('../endpoints/ops')
    const body = await bodyOf(() => wireOpsEndSilence('a1b2c3', 'fixed'))
    expect(body).toEqual(fixture('ops-silence-end'))
    expect(capturedRequest().path).toContain('/admin/ops/silences/a1b2c3/end')
  })

  it('start maintenance', async () => {
    const { wireOpsMaintenance } = await import('../endpoints/ops')
    const body = await bodyOf(() => wireOpsMaintenance('start', 'upgrade', 7200))
    expect(body).toEqual(fixture('ops-maintenance-start'))
    expect(capturedRequest().path).toContain('/admin/ops/maintenance')
  })
})
