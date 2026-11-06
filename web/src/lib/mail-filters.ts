import type { MailListFilters } from '@/lib/query-keys'
import type { getDefaultStore } from 'jotai'

import { type MailListId, threadAxesOf, type ThreadListAxes } from '@/lib/mail-lists'
import {
  followupDaysAtom,
  followupDueBy,
  type InboxSection,
  inboxSectionAtom,
  sectionedInboxAtom,
} from '@/store/inbox-sections'
import { hideSuspectedFraudAtom } from '@/store/settings'
import {
  activeLabelAtom,
  activeListAtom,
  categoryFilterAtom,
  searchQueryAtom,
  selectedAccountsAtom,
  selectedDomainsAtom,
} from '@/store/ui'

/** What decides the `/conversations` filter of the list on screen. */
export type MailFilterInputs = {
  accounts: null | string[]
  category: null | string
  domains: string[]
  followupDays: number
  hideSuspectedFraud: boolean
  inboxSection: InboxSection
  label: null | string
  list: MailListId
  search: string
  sectioned: boolean
}

type Store = ReturnType<typeof getDefaultStore>

/** The inputs as `store` holds them now, the search as typed. */
export function mailFilterInputsIn(store: Store): MailFilterInputs {
  return {
    accounts: store.get(selectedAccountsAtom),
    category: store.get(categoryFilterAtom),
    domains: store.get(selectedDomainsAtom),
    followupDays: store.get(followupDaysAtom),
    hideSuspectedFraud: store.get(hideSuspectedFraudAtom),
    inboxSection: store.get(inboxSectionAtom),
    label: store.get(activeLabelAtom),
    list: store.get(activeListAtom),
    search: store.get(searchQueryAtom),
    sectioned: store.get(sectionedInboxAtom),
  }
}

/**
 * The `/conversations` filter for the list on screen, or `null` when that
 * list is not served by `/conversations`.
 *
 * One function for the list's hook and the `/mail` route preload, so the
 * preload asks for the page the list reads.
 */
export function mailFiltersOf(i: MailFilterInputs): MailListFilters | null {
  const axes = sectionAxes(labelAxes(threadAxesOf(i.list), i.list, i.label), i.list, {
    days: i.followupDays,
    on: i.sectioned,
    section: i.inboxSection,
  })
  if (!axes) return null
  // The list fixes its axes; category, section, search and domains are
  // refinements stacked on top of whichever list is showing.
  return {
    ...axes,
    // `null` passes through as `null`: no filter. It is deliberately
    // not collapsed with `[]`, which is somebody who unticked every
    // account and must not be served the unfiltered list.
    accounts: i.accounts,
    category: i.category,
    domains: i.domains.length > 0 ? i.domains : undefined,
    // The setting says whether to hide them; the request asks for
    // them. Two ideas, and the negation is where they meet — a
    // reader who has switched hiding off is asking to be shown what
    // is held, marked, in place.
    includeQuarantined: !i.hideSuspectedFraud,
    query: i.search || undefined,
  }
}

/** The `label` list's axes carry the label it shows. */
function labelAxes(
  axes: null | ThreadListAxes,
  list: MailListId,
  label: null | string
): null | ThreadListAxes {
  if (list !== 'label' || !axes) return axes
  if (!label) return null
  return { ...axes, label }
}

/**
 * With the sectioned inbox on, the Inbox shows one section rather than the
 * folder: the server reads it off its own axis, across folders. A
 * follow-up is a waiting conversation older than the reader's days.
 */
function sectionAxes(
  axes: null | ThreadListAxes,
  list: MailListId,
  inbox: { days: number; on: boolean; section: InboxSection }
): null | ThreadListAxes {
  if (list !== 'inbox' || !inbox.on || !axes) return axes
  if (inbox.section === 'followup') {
    return { followupDueBy: followupDueBy(inbox.days, Date.now()), section: 'awaiting' }
  }
  return { section: inbox.section }
}
