import type { ListAxes } from '@/lib/list-membership'
import type { ConversationSummary } from '@/lib/types'

import { describe, expect, it } from 'vitest'

import { belongsTo, bucketOf } from '@/lib/list-membership'

function axes(over: Partial<ListAxes> = {}): ListAxes {
  return { archived: false, folder: null, starred: null, unread: null, ...over }
}

function row(over: Partial<ConversationSummary> = {}): ConversationSummary {
  return {
    archived: false,
    category: 'inbox',
    flagged: false,
    importance_level: 'normal',
    importance_score: 0,
    last_date: 0,
    message_count: 1,
    participants: [],
    pinned: false,
    received_count: 1,
    requires_action: false,
    sent_count: 0,
    snippet: '',
    subject: 's',
    thread_id: 't',
    unread_count: 0,
    ...over,
  }
}

describe('bucketOf', () => {
  /**
   * Pinned against `keys::bucket_of`
   * (`crates/mailbox-kevy/src/keys/threads.rs:228`). Both spellings of
   * each name are there because the server accepts both, and a category
   * this does not know falls to the inbox exactly as it does.
   */
  it('matches the server, singular and plural alike', () => {
    expect(bucketOf('spam')).toBe('junk')
    expect(bucketOf('scam')).toBe('junk')
    expect(bucketOf('notification')).toBe('notifications')
    expect(bucketOf('notifications')).toBe('notifications')
    expect(bucketOf('promotion')).toBe('promotions')
    expect(bucketOf('promotions')).toBe('promotions')
    expect(bucketOf('personal')).toBe('inbox')
    expect(bucketOf('SPAM')).toBe('junk')
  })
})

describe('belongsTo', () => {
  it('archived is exclusive in both directions', () => {
    expect(belongsTo(axes(), row({ archived: true }))).toBe(false)
    expect(belongsTo(axes({ archived: true }), row({ archived: true }))).toBe(true)
    expect(belongsTo(axes({ archived: true }), row())).toBe(false)
  })

  it('an archived thread is in no folder', () => {
    for (const folder of ['Inbox', 'Junk', 'NP', 'NonJunk']) {
      expect(belongsTo(axes({ folder }), row({ archived: true }))).toBe(false)
    }
  })

  it('scopes a folder to its bucket', () => {
    expect(belongsTo(axes({ folder: 'Inbox' }), row({ category: 'personal' }))).toBe(true)
    expect(belongsTo(axes({ folder: 'Inbox' }), row({ category: 'spam' }))).toBe(false)
    expect(belongsTo(axes({ folder: 'Junk' }), row({ category: 'spam' }))).toBe(true)
    expect(belongsTo(axes({ folder: 'NP' }), row({ category: 'promotion' }))).toBe(true)
    expect(belongsTo(axes({ folder: 'NP' }), row({ category: 'personal' }))).toBe(false)
  })

  it('NonJunk is everything but Junk', () => {
    expect(belongsTo(axes({ folder: 'NonJunk' }), row({ category: 'promotion' }))).toBe(true)
    expect(belongsTo(axes({ folder: 'NonJunk' }), row({ category: 'spam' }))).toBe(false)
  })

  /** The server reads the folder case-insensitively; so does this. */
  it('does not care how the folder is spelled', () => {
    expect(belongsTo(axes({ folder: 'junk' }), row({ category: 'spam' }))).toBe(true)
    expect(belongsTo(axes({ folder: 'INBOX' }), row({ category: 'spam' }))).toBe(false)
  })

  it('starred keeps only starred threads', () => {
    expect(belongsTo(axes({ starred: true }), row())).toBe(false)
    expect(belongsTo(axes({ starred: true }), row({ flagged: true }))).toBe(true)
  })

  /**
   * Unread is not a membership axis here on purpose: a thread marked
   * read while the Unread list is open stays visible until you leave it,
   * and `narrowConversations` is what implements that. Evicting the row
   * from the cache would fight it.
   */
  it('leaves the unread axis to the sticky-unread set', () => {
    expect(belongsTo(axes({ unread: true }), row({ unread_count: 0 }))).toBe(true)
  })
})
