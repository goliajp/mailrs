import type { ConversationSummary } from '@/lib/types'

import { describe, expect, it } from 'vitest'

import { latestSenders } from '@/lib/latest-senders'
import { belongsTo, type ListAxes } from '@/lib/list-membership'
import { followupDueBy } from '@/store/inbox-sections'

function axes(over: Partial<ListAxes> = {}): ListAxes {
  return { archived: false, folder: null, starred: null, trashed: false, unread: null, ...over }
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

describe('a section of the inbox', () => {
  it('holds the live rows in that section, from any folder', () => {
    const reply = axes({ section: 'reply' })
    expect(belongsTo(reply, row({ section: 'reply' }))).toBe(true)
    expect(belongsTo(reply, row({ category: 'promotion', section: 'reply' }))).toBe(true)
    expect(belongsTo(reply, row({ section: 'other' }))).toBe(false)
    expect(belongsTo(reply, row({ archived: true, section: 'reply' }))).toBe(false)
    expect(belongsTo(reply, row({ section: 'reply', trashed_at: 5 }))).toBe(false)
    expect(belongsTo(reply, row())).toBe(false)
  })

  it('a follow-up is a waiting row whose wait began by the due time', () => {
    const due = axes({ followupDueBy: 1000, section: 'awaiting' })
    expect(belongsTo(due, row({ awaiting_since: 900, section: 'awaiting' }))).toBe(true)
    expect(belongsTo(due, row({ awaiting_since: 1100, section: 'awaiting' }))).toBe(false)
  })

  it('is due days before now, to the hour', () => {
    const now = Date.UTC(2026, 10, 1, 10, 42) // 10:42
    const hour = Date.UTC(2026, 10, 1, 10, 0) / 1000
    expect(followupDueBy(3, now)).toBe(hour - 3 * 86_400)
    expect(followupDueBy(3, now + 60_000)).toBe(followupDueBy(3, now))
  })
})

describe('the senders to accept', () => {
  it('are each conversation latest correspondent other than me, once each', () => {
    const rows = [
      { participants: ['Eve <EVE@far.test>'] },
      { participants: ['Eve <eve@far.test>', 'Me <me@x.test>'] },
      { participants: ['zed@far.test', 'Ann <ann@y.test>'] },
      { participants: ['me@x.test'] },
    ]
    expect(latestSenders(rows, 'me@x.test')).toEqual(['eve@far.test', 'ann@y.test'])
  })
})
