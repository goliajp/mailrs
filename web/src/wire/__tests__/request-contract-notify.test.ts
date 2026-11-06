import { describe, expect, it } from 'vitest'

import { bodyOf, fixture } from './request-contract-harness'

describe('sender rule and push request bodies match the shared contract', () => {
  it('notify-prefs-set', async () => {
    const { wirePutNotifyPrefs } = await import('../endpoints/notify')
    const body = await bodyOf(() =>
      wirePutNotifyPrefs({
        hide_content: true,
        mode: 'important',
        quiet_end: 420,
        quiet_start: 1320,
        utc_offset_minutes: 540,
      })
    )
    expect(body).toEqual(fixture('notify-prefs-set'))
  })

  it('notify-override, for a sender and for a conversation', async () => {
    const { wireNotifySender } = await import('../endpoints/notify')
    const { wireNotifyThread } = await import('../endpoints/notify')
    expect(await bodyOf(() => wireNotifySender('boss@x.test', 'always'))).toEqual(
      fixture('notify-override')
    )
    expect(await bodyOf(() => wireNotifyThread('t1', 'mute'))).toEqual({ value: 'mute' })
  })

  it('important-set', async () => {
    const { wireSetImportant } = await import('../endpoints/notify')
    expect(await bodyOf(() => wireSetImportant('boss@x.test', true))).toEqual(
      fixture('important-set')
    )
  })

  it('sender-reclassify, sender-rule-set', async () => {
    const { wireReclassifySender } = await import('../endpoints/sender-rules')
    const { wirePutSenderRule } = await import('../endpoints/sender-rules')
    expect(await bodyOf(() => wireReclassifySender('news@shop.test', 'notifications'))).toEqual(
      fixture('sender-reclassify')
    )
    const rule = await bodyOf(() =>
      wirePutSenderRule('news@shop.test', {
        bucket: 'promotions',
        disposal: 'trash',
        keep_latest: 3,
        older_than_days: 30,
      })
    )
    expect(rule).toEqual(fixture('sender-rule-set'))
  })

  it('web-push-subscribe, web-push-unsubscribe', async () => {
    const { wireWebPushSubscribe } = await import('../endpoints/web-push')
    const { wireWebPushUnsubscribe } = await import('../endpoints/web-push')
    const sub = fixture('web-push-subscribe') as {
      endpoint: string
      keys: { auth: string; p256dh: string }
      resume: true
    }
    expect(await bodyOf(() => wireWebPushSubscribe(sub))).toEqual(sub)
    expect(await bodyOf(() => wireWebPushUnsubscribe(sub.endpoint))).toEqual(
      fixture('web-push-unsubscribe')
    )
  })
})
