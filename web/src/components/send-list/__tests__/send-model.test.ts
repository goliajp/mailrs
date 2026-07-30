import type { WireSentMessage } from '@/wire/schemas/mail'
import type { WireSend } from '@/wire/schemas/sends'

import { describe, expect, it } from 'vitest'

import {
  failedRecipients,
  filterByStatus,
  indexSendsByMessage,
  joinKey,
  joinSends,
  needsAttention,
} from '../send-model'

function msg(over: Partial<WireSentMessage> & { message_id: string }): WireSentMessage {
  return {
    internal_date: 1785369273,
    subject: 's',
    thread_id: over.message_id,
    to: 'GOLIA <goliaaccess@gmail.com>',
    uid: 1,
    ...over,
  }
}

function send(over: Partial<WireSend> & { send_id: string }): WireSend {
  return {
    can_resend: true,
    created_at: 1785369273,
    recipients: [],
    resent_from: null,
    status: 'delivered',
    subject: 's',
    thread_id: over.send_id,
    to: ['GOLIA <goliaaccess@gmail.com>'],
    ...over,
  }
}

describe('joinKey', () => {
  /// Both sides store the id bare today, verified against prod captures
  /// on 2026-07-30. Normalising anyway means a future change on either
  /// side does not silently strip every badge from the list.
  it('matches the same id however it is written', () => {
    expect(joinKey('<A@B.com>')).toBe(joinKey(' a@b.com '))
  })
})

describe('joinSends', () => {
  it('leaves a message with no send record without one', () => {
    const rows = joinSends([msg({ message_id: 'old@golia.jp' })], [])
    expect(rows).toHaveLength(1)
    expect(rows[0].send).toBeNull()
  })

  /// The whole reason status is an enrichment: every send before the
  /// projection shipped has no row, and the list still has to show them.
  it('keeps history visible alongside rows that do have status', () => {
    const rows = joinSends(
      [msg({ message_id: 'new@golia.jp' }), msg({ message_id: 'ancient@golia.jp' })],
      [send({ send_id: 'new@golia.jp', status: 'failed' })]
    )
    expect(rows).toHaveLength(2)
    expect(rows[0].send?.status).toBe('failed')
    expect(rows[1].send).toBeNull()
  })
})

describe('indexSendsByMessage', () => {
  /// A resend's own id carries `#r<n>`, so joining on it would never
  /// match the message. `resent_from` names the send it repeats.
  it('files a resend under the message it repeats', () => {
    const idx = indexSendsByMessage([
      send({ created_at: 100, send_id: 'a@golia.jp', status: 'failed' }),
      send({
        created_at: 200,
        resent_from: 'a@golia.jp',
        send_id: 'a@golia.jp#r1',
        status: 'delivered',
      }),
    ])
    expect(idx.size).toBe(1)
    expect(idx.get('a@golia.jp')?.status).toBe('delivered')
  })

  /// "Where does this mail stand now", not "how did the first try go".
  it('keeps the latest attempt when they arrive out of order', () => {
    const idx = indexSendsByMessage([
      send({
        created_at: 300,
        resent_from: 'a@golia.jp',
        send_id: 'a@golia.jp#r2',
        status: 'delivered',
      }),
      send({ created_at: 100, send_id: 'a@golia.jp', status: 'failed' }),
    ])
    expect(idx.get('a@golia.jp')?.status).toBe('delivered')
  })
})

describe('needsAttention', () => {
  /// Built through `joinSends` rather than from a hand-written row: a
  /// literal would keep compiling after the row shape changes and stop
  /// describing anything real.
  function rowFor(status: null | WireSend['status']) {
    const sends = status === null ? [] : [send({ send_id: 'a@golia.jp', status })]
    return joinSends([msg({ message_id: 'a@golia.jp' })], sends)[0]
  }

  it('flags failed and partial, and nothing else', () => {
    expect(needsAttention(rowFor(null))).toBe(false)
    for (const status of ['scheduled', 'sending', 'delivered'] as const) {
      expect(needsAttention(rowFor(status)), status).toBe(false)
    }
    for (const status of ['failed', 'partial'] as const) {
      expect(needsAttention(rowFor(status)), status).toBe(true)
    }
  })
})

describe('failedRecipients', () => {
  /// A partial send is the case that matters: the detail has to name who
  /// missed out, and listing the ones that landed buries it.
  it('names only the recipients that did not make it', () => {
    const s = send({
      recipients: [
        { code: 250, delivered: true, message: '', pending: false, recipient: 'ok@x.com' },
        {
          code: 550,
          delivered: false,
          message: '5.1.1 no such user',
          pending: false,
          recipient: 'nope@x.com',
        },
        { code: 0, delivered: false, message: '', pending: true, recipient: 'later@x.com' },
      ],
      send_id: 'a@golia.jp',
      status: 'partial',
    })
    const failed = failedRecipients(s)
    expect(failed).toHaveLength(1)
    expect(failed[0].recipient).toBe('nope@x.com')
    expect(failed[0].message).toBe('5.1.1 no such user')
  })
})

describe('filterByStatus', () => {
  /// A row with no record has no status. Sweeping it into a bucket would
  /// state an outcome nobody recorded.
  it('excludes records-less rows from every status filter', () => {
    const rows = joinSends(
      [msg({ message_id: 'new@golia.jp' }), msg({ message_id: 'ancient@golia.jp' })],
      [send({ send_id: 'new@golia.jp', status: 'delivered' })]
    )
    expect(filterByStatus(rows, null)).toHaveLength(2)
    expect(filterByStatus(rows, 'delivered')).toHaveLength(1)
    expect(filterByStatus(rows, 'failed')).toHaveLength(0)
  })
})

describe('joinSends as a full outer join', () => {
  /**
   * The reported bug (2026-07-30): a reply to nagata@nagatax.tokyo.jp was
   * accepted with a 250, had a Send row, and had its maildir copy — and did
   * not appear in Send, because nothing on the ingest path writes the sent
   * axis. Its only writer is fastcore's periodic maildir sweep, which backs
   * off exponentially while idle.
   */
  it('shows a send the sweep has not filed yet', () => {
    const rows = joinSends(
      [],
      [
        send({
          created_at: 1785388821,
          send_id: '9d8549f828cd6aea@golia.jp',
          status: 'delivered',
          subject: 'Re: 決算について',
          to: ['nagata@nagatax.tokyo.jp'],
        }),
      ]
    )
    expect(rows).toHaveLength(1)
    expect(rows[0].subject).toBe('Re: 決算について')
    expect(rows[0].to).toBe('nagata@nagatax.tokyo.jp')
    expect(rows[0].send?.status).toBe('delivered')
    expect(rows[0].msg).toBeNull()
    // Null, not 0: the maildir copy is not indexed, and 0 is a real uid
    // shape that would make the thread view chase a message that is not
    // there.
    expect(rows[0].uid).toBeNull()
  })

  it('does not duplicate a send that both sources know about', () => {
    const rows = joinSends(
      [msg({ message_id: 'a@golia.jp', subject: 'once' })],
      [send({ send_id: 'a@golia.jp', subject: 'once' })]
    )
    expect(rows).toHaveLength(1)
    expect(rows[0].msg).not.toBeNull()
    expect(rows[0].send).not.toBeNull()
  })

  /// History has no Send row and must still be listed, which is why the
  /// projection cannot simply replace the axis.
  it('keeps axis-only history alongside projection-only sends', () => {
    const rows = joinSends(
      [msg({ internal_date: 100, message_id: 'ancient@golia.jp' })],
      [send({ created_at: 200, send_id: 'fresh@golia.jp' })]
    )
    expect(rows.map((r) => r.messageId)).toEqual(['fresh@golia.jp', 'ancient@golia.jp'])
    expect(rows[1].send).toBeNull()
  })

  it('sorts newest first across both sources', () => {
    const rows = joinSends(
      [
        msg({ internal_date: 300, message_id: 'b@golia.jp' }),
        msg({ internal_date: 100, message_id: 'd@golia.jp' }),
      ],
      [
        send({ created_at: 400, send_id: 'a@golia.jp' }),
        send({ created_at: 200, send_id: 'c@golia.jp' }),
      ]
    )
    expect(rows.map((r) => r.messageId)).toEqual([
      'a@golia.jp',
      'b@golia.jp',
      'c@golia.jp',
      'd@golia.jp',
    ])
  })

  /// A resend files under the message it repeats, so it must not add a
  /// second row for the same mail.
  it('does not add a row for a resend of a listed message', () => {
    const rows = joinSends(
      [msg({ message_id: 'a@golia.jp' })],
      [
        send({ created_at: 100, send_id: 'a@golia.jp', status: 'failed' }),
        send({
          created_at: 200,
          resent_from: 'a@golia.jp',
          send_id: 'a@golia.jp#r1',
          status: 'delivered',
        }),
      ]
    )
    expect(rows).toHaveLength(1)
    expect(rows[0].send?.status).toBe('delivered')
  })
})
