import { describe, expect, it } from 'vitest'

import { bodyOf, capturedRequest, fixture } from './request-contract-harness'

const c = { conv: 41, gid: 3 }

describe('team inbox request bodies match the shared contract', () => {
  it('shared-assign', async () => {
    const { wireAssignShared } = await import('../endpoints/shared')
    expect(await bodyOf(() => wireAssignShared(c, 'bob@example.test'))).toEqual(
      fixture('shared-assign')
    )
    expect(capturedRequest()).toMatchObject({ method: 'POST', path: '/api/shared/3/41/assign' })
  })

  it('shared-note, for adding and editing', async () => {
    const { wireAddNote } = await import('../endpoints/shared')
    const { wireEditNote } = await import('../endpoints/shared')
    const text = 'Refund approved, call them back'
    expect(await bodyOf(() => wireAddNote(c, text))).toEqual(fixture('shared-note'))
    expect(capturedRequest().method).toBe('POST')
    expect(await bodyOf(() => wireEditNote(c, 9, text))).toEqual(fixture('shared-note'))
    expect(capturedRequest()).toMatchObject({ method: 'PUT', path: '/api/shared/3/41/notes/9' })
  })

  it('shared-presence', async () => {
    const { wireSharedHeartbeat } = await import('../endpoints/shared')
    expect(await bodyOf(() => wireSharedHeartbeat(c, 'composing'))).toEqual(
      fixture('shared-presence')
    )
  })

  it('shared-draft-announce', async () => {
    const { wireAnnounceDraft } = await import('../endpoints/shared')
    expect(await bodyOf(() => wireAnnounceDraft(c, 7))).toEqual(fixture('shared-draft-announce'))
    expect(capturedRequest().method).toBe('PUT')
  })

  it('email-group-shared-set', async () => {
    const { wireSetGroupShared } = await import('../endpoints/shared')
    expect(await bodyOf(() => wireSetGroupShared(5, true))).toEqual(
      fixture('email-group-shared-set')
    )
    expect(capturedRequest()).toMatchObject({
      method: 'PUT',
      path: '/api/admin/email-groups/5/shared',
    })
  })

  it('send-team-reply', async () => {
    const { sendMail } = await import('@/lib/send-mail')
    const body = await bodyOf(() =>
      sendMail({
        bcc: [],
        body: 'hello',
        cc: [],
        from: 'team@golia.jp',
        htmlBody: '<p>hello</p>',
        replyToThreadId: 't1',
        shared: { conv: 41, gid: 3, seenSeq: 2 },
        subject: 'Re: order',
        to: ['customer@example.com'],
      })
    )
    expect(body).toEqual(fixture('send-team-reply'))
  })
})

describe('a team reply refused because another member answered first', () => {
  it('comes back as already-replied, not as a failed send', async () => {
    const { vi } = await import('vitest')
    vi.stubGlobal(
      'fetch',
      vi.fn(() =>
        Promise.resolve(
          new Response(
            JSON.stringify({
              at: 1_790_000_000,
              by: 'a@golia.jp',
              error: 'already_replied',
              seq: 3,
            }),
            { headers: { 'content-type': 'application/json' }, status: 409 }
          )
        )
      )
    )
    const { sendMail } = await import('@/lib/send-mail')
    const r = await sendMail({
      body: 'x',
      from: 'team@golia.jp',
      htmlBody: 'x',
      shared: { conv: 41, gid: 3, seenSeq: 2 },
      subject: 's',
      to: ['customer@example.com'],
    })
    expect(r).toEqual({ alreadyReplied: { at: 1_790_000_000, by: 'a@golia.jp' }, success: false })
  })
})
