import { describe, expect, it } from 'vitest'

import {
  mockFullMessageSchema,
  mockMessageListSchema,
  mockNamespaceListSchema,
  mockSendResultSchema,
  mockSmtpCredentialsSchema,
} from '../schemas/mock'

/**
 * Written from the Rust structs, not captured: the feature is not deployed
 * yet. Each object is what serde produces for that struct, flattening
 * included — `NamespaceView` carries `Namespace`'s fields at the top level,
 * `MessageMeta` carries `Summary`'s, `FullMessage` carries `MessageMeta`'s.
 */
const MESSAGE_META = {
  attachments: [{ content_type: 'application/pdf', filename: 'a.pdf', size: 1234 }],
  cc: [],
  codes: ['482913'],
  direction: 'in',
  from: 'GitHub <noreply@github.com>',
  id: '9f86d081884c7d65',
  links: [{ text: 'Verify', url: 'https://github.com/login/device/verify?token=abc' }],
  message_id: '<abc@github.com>',
  namespace: 'insight',
  rcpt: 'insight.signup-0417@mock.golia.jp',
  received_at: 1_792_195_200_123,
  size: 5321,
  subject: '[GitHub] Please verify your device',
  tag: 'signup-0417',
  to: ['insight.signup-0417@mock.golia.jp'],
}

describe('mock mail schemas', () => {
  it('parses the namespace list', () => {
    const parsed = mockNamespaceListSchema.parse({
      namespaces: [
        {
          address: 'insight.test@mock.golia.jp',
          created_at: 1_792_195_000_000,
          dropped: 3,
          messages: 12,
          name: 'insight',
          owner: 'lihao@golia.jp',
          smtp_enabled: false,
        },
      ],
    })
    expect(parsed.namespaces[0].address).toBe('insight.test@mock.golia.jp')
    expect(parsed.namespaces[0].dropped).toBe(3)
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

  it('parses a full message, html absent', () => {
    const parsed = mockFullMessageSchema.parse({
      ...MESSAGE_META,
      dropped: 0,
      html: null,
      text: 'Your code is 482913',
    })
    expect(parsed.codes).toEqual(['482913'])
    expect(parsed.links[0].text).toBe('Verify')
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
