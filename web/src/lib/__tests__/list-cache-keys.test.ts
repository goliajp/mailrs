import type { MailListId } from '@/lib/mail-lists'
import type { MailListFilters } from '@/lib/query-keys'
import type { ConversationSummary } from '@/lib/types'

import { afterEach, describe, expect, it } from 'vitest'

import { conversationListKey } from '@/lib/list-axes'
import { patchConversationFields } from '@/lib/mail-cache'
import { type MailFilterInputs, mailFiltersOf } from '@/lib/mail-filters'
import { MAIL_LIST_TABS, threadAxesOf } from '@/lib/mail-lists'
import { queryClient } from '@/lib/query-client'
import { INBOX_SECTIONS, type InboxSection } from '@/store/inbox-sections'

function filtersOf(over: Partial<MailFilterInputs>): MailListFilters {
  const f = mailFiltersOf(inputs(over))
  if (!f) throw new Error(`no /conversations filter for ${JSON.stringify(over)}`)
  return f
}

/**
 * Every list keeps its rows in a cache line of its own.
 *
 * The key used to drop `pinned`, `snoozed`, `section` and `followupDueBy`,
 * so Snoozed, Waiting and every section of the sectioned inbox shared one
 * line with the unscoped list: switching to one showed another list's rows
 * until the refetch landed, and a cache update could not tell the lists
 * apart either, since it reads a line's axes back off its key.
 */
function inputs(over: Partial<MailFilterInputs>): MailFilterInputs {
  return {
    accounts: null,
    category: null,
    domains: [],
    followupDays: 3,
    hideSuspectedFraud: true,
    inboxSection: 'reply',
    label: 'l1',
    list: 'inbox',
    search: '',
    sectioned: false,
    ...over,
  }
}

const keyOf = (f: MailListFilters) => JSON.stringify(conversationListKey(f))

const tabs: MailListId[] = MAIL_LIST_TABS.filter((id) => threadAxesOf(id) !== null)

function idsIn(f: MailListFilters): string[] {
  const data = queryClient.getQueryData<{ pages: ConversationSummary[][] }>(conversationListKey(f))
  return (data?.pages ?? []).flat().map((c) => c.thread_id)
}

function row(over: Partial<ConversationSummary>): ConversationSummary {
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
    thread_id: 't1',
    unread_count: 0,
    ...over,
  }
}

function seed(f: MailListFilters, rows: ConversationSummary[]) {
  queryClient.setQueryData(conversationListKey(f), { pageParams: [undefined], pages: [rows] })
}

afterEach(() => queryClient.clear())

describe('list cache keys', () => {
  it('gives every list tab a key of its own', () => {
    const keys = tabs.map((list) => keyOf(filtersOf({ list })))
    expect(tabs.length).toBeGreaterThan(10)
    expect(new Set(keys).size).toBe(tabs.length)
  })

  it('gives every inbox section a key of its own, apart from every tab', () => {
    const sections = INBOX_SECTIONS.map((inboxSection: InboxSection) =>
      keyOf(filtersOf({ inboxSection, list: 'inbox', sectioned: true }))
    )
    const plain = tabs.map((list) => keyOf(filtersOf({ list })))
    expect(new Set(sections).size).toBe(INBOX_SECTIONS.length)
    for (const k of sections) expect(plain).not.toContain(k)
  })

  it('with no section chosen, asks for the whole inbox as it does with sections off', () => {
    const none = filtersOf({ inboxSection: null, list: 'inbox', sectioned: true })
    expect(none.section).toBeUndefined()
    expect(none.followupDueBy).toBeUndefined()
    expect(keyOf(none)).toBe(keyOf(filtersOf({ list: 'inbox', sectioned: false })))
  })

  it('keys the follow-up section by how long it has waited', () => {
    const a = keyOf(filtersOf({ followupDays: 3, inboxSection: 'followup', sectioned: true }))
    const b = keyOf(filtersOf({ followupDays: 7, inboxSection: 'followup', sectioned: true }))
    expect(a).not.toBe(b)
  })
})

describe('cache updates reach the list they belong to', () => {
  it('a conversation woken from snooze leaves Snoozed and stays in All Mail', () => {
    const snoozed = filtersOf({ list: 'snoozed' })
    const all = filtersOf({ list: 'all' })
    const away = row({ snoozed_until: Date.now() / 1000 + 3600 })
    seed(snoozed, [away])
    seed(all, [away])

    patchConversationFields((c) => ({ ...c, snoozed_until: 0 }))

    expect(idsIn(snoozed)).toEqual([])
    expect(idsIn(all)).toEqual(['t1'])
  })

  it('a conversation moving between sections leaves one and is kept by the other', () => {
    const reply = filtersOf({ inboxSection: 'reply', sectioned: true })
    const other = filtersOf({ inboxSection: 'other', sectioned: true })
    seed(reply, [row({ section: 'reply', thread_id: 'r' })])
    seed(other, [row({ section: 'other', thread_id: 'o' })])

    patchConversationFields((c) => ({ ...c, section: 'other' }))

    expect(idsIn(reply)).toEqual([])
    expect(idsIn(other)).toEqual(['o'])
  })

  it('an unpinned conversation leaves Pinned', () => {
    const pinned = filtersOf({ list: 'pinned' })
    seed(pinned, [row({ pinned: true })])

    patchConversationFields((c) => ({ ...c, pinned: false }))

    expect(idsIn(pinned)).toEqual([])
  })

  it('a conversation answered leaves Waiting', () => {
    const waiting = filtersOf({ list: 'awaiting' })
    seed(waiting, [row({ section: 'awaiting' })])

    patchConversationFields((c) => ({ ...c, section: 'reply' }))

    expect(idsIn(waiting)).toEqual([])
  })
})
