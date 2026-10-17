import { describe, expect, it } from 'vitest'

import {
  mockFullMessageSchema,
  mockMessageListSchema,
  mockNamespaceListSchema,
  mockSendResultSchema,
  mockSmtpCredentialsSchema,
} from '../schemas/mock'

/**
 * Captured from production on 2.95.0 (2026-10-17), plus `uid` and
 * `webhook_url`, which arrived after that release.
 */
const MESSAGE_META = {
  attachments: [],
  cc: [],
  codes: ['482913'],
  direction: 'in',
  from: 'lihao@golia.jp',
  id: 'a4944da5bfdcb2d5',
  links: [],
  message_id: '<179220630269.56984.7756892933248975240@golia.jp>',
  namespace: 'devops-check',
  rcpt: 'devops-check.t1@mock.golia.jp',
  received_at: 1_792_206_304_447,
  size: 470,
  subject: 'mock email check',
  tag: 't1',
  to: ['devops-check.t1@mock.golia.jp'],
  uid: 1,
}

describe('mock mail schemas', () => {
  it('parses the namespace list', () => {
    const parsed = mockNamespaceListSchema.parse({
      namespaces: [
        {
          address: 'devops-check.test@mock.golia.jp',
          bytes: 0,
          created_at: 1_792_206_288_897,
          dropped: 0,
          messages: 0,
          name: 'devops-check',
          owner: 'lihao@golia.jp',
          smtp_enabled: false,
          webhook_url: null,
        },
      ],
    })
    expect(parsed.namespaces[0].address).toBe('devops-check.test@mock.golia.jp')
    expect(parsed.namespaces[0].webhook_url).toBeNull()
  })

  it('parses a message list with both directions', () => {
    const parsed = mockMessageListSchema.parse({
      dropped: 0,
      messages: [
        MESSAGE_META,
        { ...MESSAGE_META, direction: 'captured-out', id: 'm1', message_id: null },
      ],
    })
    expect(parsed.messages.map((m) => m.direction)).toEqual(['in', 'captured-out'])
    expect(parsed.messages[1].message_id).toBeNull()
  })

  it('rejects a direction the backend does not have', () => {
    const bad = { dropped: 0, messages: [{ ...MESSAGE_META, direction: 'out' }] }
    expect(mockMessageListSchema.safeParse(bad).success).toBe(false)
  })

  it('parses the wait response for a full message, html absent', () => {
    const parsed = mockFullMessageSchema.parse({
      ...MESSAGE_META,
      dropped: 0,
      html: null,
      status: 'received',
      text: 'Your verification code is 482913\r\n',
    })
    expect(parsed.codes).toEqual(['482913'])
    expect(parsed.links).toEqual([])
    expect(parsed.html).toBeNull()
  })

  it('parses the send result and the SMTP credentials', () => {
    const sent = mockSendResultSchema.parse({
      delivered: ['insight.a@mock.golia.jp'],
      id: 'm18f2c3a4b5c6d7e8f9',
      not_delivered: [
        { address: 'x@example.com', reason: 'not a mock address; captured, never sent' },
      ],
    })
    expect(sent.not_delivered[0].reason).toContain('captured')

    const creds = mockSmtpCredentialsSchema.parse({
      host: 'mail.golia.jp',
      password: 'p4ssw0rd',
      username: 'insight@mock.golia.jp',
    })
    expect(creds.username).toBe('insight@mock.golia.jp')
  })
})
