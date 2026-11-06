import { describe, expect, it } from 'vitest'

import { bodyOf, fixture } from './request-contract-harness'

describe('settings request bodies match the shared contract', () => {
  it('screening-observation, screening-hash', async () => {
    const { wireAddScreeningObservation } = await import('../endpoints/screening')
    const { wireAddScreeningHash } = await import('../endpoints/screening')
    const obs = await bodyOf(() =>
      wireAddScreeningObservation({
        kind: 'ip',
        reason: '2026-10-24 incident',
        source: 'incident',
        stance: 'deny',
        value: '200.181.82.134',
      })
    )
    expect(obs).toEqual(fixture('screening-observation'))
    const hash = await bodyOf(() =>
      wireAddScreeningHash({
        reason: "the vendor's installer",
        sha256: 'ab'.repeat(32),
        stance: 'allow',
      })
    )
    expect(hash).toEqual(fixture('screening-hash'))
  })

  it('app-password-create, protocol-password-set', async () => {
    const { wireCreateAppPassword } = await import('../endpoints/app-passwords')
    const { wireSetProtocolPassword } = await import('../endpoints/protocol-logins')
    const made = await bodyOf(() => wireCreateAppPassword('iPhone Mail'))
    expect(made).toEqual(fixture('app-password-create'))
    const set = await bodyOf(() => wireSetProtocolPassword('bob@golia.jp', 'app-only'))
    expect(set).toEqual(fixture('protocol-password-set'))
  })

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

  it('swipe-set', async () => {
    const { wireSetSwipe } = await import('../endpoints/mail-settings')
    const body = await bodyOf(() => wireSetSwipe({ left: 'none', right: 'read' }))
    expect(body).toEqual(fixture('swipe-set'))
  })

  it('sectioned-inbox-set', async () => {
    const { wireSetSectionedInbox } = await import('../endpoints/mail-settings')
    const body = await bodyOf(() => wireSetSectionedInbox({ enabled: false }))
    expect(body).toEqual(fixture('sectioned-inbox-set'))
  })

  it('forwarding-request, forwarding-verify', async () => {
    const { wireRequestForwarding } = await import('../endpoints/mail-settings')
    const { wireVerifyForwarding } = await import('../endpoints/mail-settings')
    expect(await bodyOf(() => wireRequestForwarding('me@elsewhere.example', false))).toEqual(
      fixture('forwarding-request')
    )
    expect(await bodyOf(() => wireVerifyForwarding('123456'))).toEqual(fixture('forwarding-verify'))
  })

  it('signature save', async () => {
    const { wireCreateSignature } = await import('../endpoints/settings')
    const body = await bodyOf(() =>
      wireCreateSignature({
        html_content: '<p>Regards</p>',
        name: 'default',
        text_content: 'Regards',
      })
    )
    // `id` is absent in the fixture and undefined here; JSON.stringify drops
    // it, so the captured body has no such key either.
    expect(body).toEqual(fixture('signature-save'))
  })

  it('template save', async () => {
    const { wireCreateTemplate } = await import('../endpoints/templates')
    const body = await bodyOf(() =>
      wireCreateTemplate({ name: 'Weekly report', subject: 'Report', text_body: 'Hello team' })
    )
    expect(body).toEqual(fixture('template-save'))
  })
})
