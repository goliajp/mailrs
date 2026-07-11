// Smoke tests for the query-key factories. Most of these keys are simple
// tuples whose value matters only as a cache identity, so the goal here is
// to (a) lock in the shape against accidental refactors and (b) ensure the
// factories are referenced in coverage at all (otherwise unused branches
// inflate the lib/** coverage gate even though every key is used in
// production code).

import { describe, expect, it } from 'vitest'

import {
  adminKeys,
  calendarKeys,
  contactsKeys,
  dashboardKeys,
  mailKeys,
  messageKeys,
  settingsKeys,
} from '@/lib/query-keys'

describe('mailKeys', () => {
  it('is stable across repeat calls with equivalent filters', () => {
    const a = mailKeys.conversations({ archived: true, unread: true })
    const b = mailKeys.conversations({ archived: true, unread: true })
    expect(a).toEqual(b)
  })

  it('normalizes domain order', () => {
    const a = mailKeys.conversations({ domains: ['b.com', 'a.com'] })
    const b = mailKeys.conversations({ domains: ['a.com', 'b.com'] })
    expect(a).toEqual(b)
  })

  it('drops false-y filter values from the normalized key', () => {
    const empty = mailKeys.conversations({})
    const withFalse = mailKeys.conversations({ archived: false, unread: false })
    expect(empty).toEqual(withFalse)
  })

  it('produces distinct keys for distinct filters', () => {
    const inbox = mailKeys.conversations({ folder: 'INBOX' })
    const sent = mailKeys.conversations({ folder: 'SENT' })
    expect(inbox).not.toEqual(sent)
  })

  it('exposes thread / search / category factories', () => {
    expect(mailKeys.thread('t1')).toEqual(['mail', 'thread', 't1'])
    expect(mailKeys.thread(null)).toEqual(['mail', 'thread', ''])
    expect(mailKeys.search('hello', { folder: 'INBOX' })).toEqual([
      'mail',
      'search',
      'hello',
      { folder: 'INBOX' },
    ])
    expect(mailKeys.categories(['b.com', 'a.com'])).toEqual(['mail', 'categories', 'a.com,b.com'])
    expect(mailKeys.all()).toEqual(['mail'])
  })
})

describe('adminKeys', () => {
  it('namespaces every admin entry under admin/', () => {
    expect(adminKeys.all()).toEqual(['admin'])
    expect(adminKeys.domains()).toEqual(['admin', 'domains'])
    expect(adminKeys.accounts()).toEqual(['admin', 'accounts'])
    expect(adminKeys.aliases()).toEqual(['admin', 'aliases'])
    expect(adminKeys.apps()).toEqual(['admin', 'apps'])
    expect(adminKeys.auditLog()).toEqual(['admin', 'audit-log'])
    expect(adminKeys.emailGroups()).toEqual(['admin', 'email-groups'])
    expect(adminKeys.groups()).toEqual(['admin', 'groups'])
    expect(adminKeys.permissions()).toEqual(['admin', 'permissions'])
    expect(adminKeys.queues()).toEqual(['admin', 'queues'])
    expect(adminKeys.systemConfig()).toEqual(['admin', 'system-config'])
    expect(adminKeys.mailAuditAccounts()).toEqual(['admin', 'mail-audit-accounts'])
    expect(adminKeys.mailAuditConversations('a@b')).toEqual([
      'admin',
      'mail-audit-conversations',
      'a@b',
    ])
    expect(adminKeys.mailAuditThread('a@b', 'tid')).toEqual([
      'admin',
      'mail-audit-thread',
      'a@b',
      'tid',
    ])
    expect(adminKeys.overviewHealth()).toEqual(['admin', 'overview-health'])
    expect(adminKeys.overviewStatus()).toEqual(['admin', 'overview-status'])
    expect(adminKeys.overviewSmtp()).toEqual(['admin', 'overview-smtp'])
    expect(adminKeys.overviewAuditLog()).toEqual(['admin', 'overview-audit-log'])
  })
})

describe('settingsKeys', () => {
  it('namespaces every settings entry under settings/', () => {
    expect(settingsKeys.all()).toEqual(['settings'])
    expect(settingsKeys.signatures()).toEqual(['settings', 'signatures'])
    expect(settingsKeys.agentKeys()).toEqual(['settings', 'agent-keys'])
    expect(settingsKeys.calendarFeeds()).toEqual(['settings', 'calendar-feeds'])
    expect(settingsKeys.encryptionKeysStatus()).toEqual(['settings', 'encryption-keys-status'])
    expect(settingsKeys.recoveryEmail()).toEqual(['settings', 'recovery-email'])
    expect(settingsKeys.totpStatus()).toEqual(['settings', 'totp-status'])
    expect(settingsKeys.webhooks()).toEqual(['settings', 'webhooks'])
  })
})

describe('dashboardKeys', () => {
  it('namespaces every dashboard entry under dashboard/', () => {
    expect(dashboardKeys.all()).toEqual(['dashboard'])
    expect(dashboardKeys.conversations()).toEqual(['dashboard', 'conversations'])
    expect(dashboardKeys.stats()).toEqual(['dashboard', 'stats'])
    expect(dashboardKeys.folders()).toEqual(['dashboard', 'folders'])
  })
})

describe('calendarKeys', () => {
  it('includes the conflict window in the key', () => {
    expect(calendarKeys.all()).toEqual(['calendar'])
    expect(calendarKeys.conflicts('2026-01-01T00:00:00Z', '2026-01-01T01:00:00Z', 'uid-1')).toEqual(
      ['calendar', 'conflicts', '2026-01-01T00:00:00Z', '2026-01-01T01:00:00Z', 'uid-1']
    )
  })
})

describe('messageKeys', () => {
  it('namespaces detail by uid', () => {
    expect(messageKeys.all()).toEqual(['message'])
    expect(messageKeys.detail(42)).toEqual(['message', 'detail', 42])
  })
})

describe('contactsKeys', () => {
  it('namespaces search by query', () => {
    expect(contactsKeys.all()).toEqual(['contacts'])
    expect(contactsKeys.search('alice')).toEqual(['contacts', 'search', 'alice'])
  })
})
