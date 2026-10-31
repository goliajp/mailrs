import { describe, expect, it } from 'vitest'

import { bodyOf, fixture } from './request-contract-harness'

describe('settings request bodies match the shared contract', () => {
  it('vacation-set', async () => {
    const { wireSetVacation } = await import('../endpoints/mail-settings')
    const body = await bodyOf(() =>
      wireSetVacation({
        body: 'Back on Monday.',
        days: 7,
        enabled: true,
        end: 1_793_500_000,
        start: null,
        subject: 'Away',
      })
    )
    expect(body).toEqual(fixture('vacation-set'))
  })

  it('forwarding-request, forwarding-verify', async () => {
    const { wireRequestForwarding } = await import('../endpoints/mail-settings')
    const { wireVerifyForwarding } = await import('../endpoints/mail-settings')
    expect(await bodyOf(() => wireRequestForwarding('me@elsewhere.example', false))).toEqual(
      fixture('forwarding-request')
    )
    expect(await bodyOf(() => wireVerifyForwarding('123456'))).toEqual(fixture('forwarding-verify'))
  })
})
