/**
 * The `/conversations` filter for the list on screen — or `null` when
 * the list on screen is not served by `/conversations` at all.
 *
 * `null` rather than a default is the point: Send and Draft have their
 * own endpoints, and returning an Inbox filter for them is exactly how
 * `chat.tsx` ended up running a second conversation query and selecting
 * its first row while the Send list was the thing on screen.
 *
 * The rows themselves, and the selection over them, live in
 * `use-current-list` — which depends on this. Nothing here may depend
 * back on it.
 */

import { useQuery } from '@tanstack/react-query'
import { useAtomValue } from 'jotai'
import { useMemo } from 'react'

import { useDebouncedValue } from '@/hooks/use-debounced-value'
import { type MailListId, threadAxesOf, type ThreadListAxes } from '@/lib/mail-lists'
import { type MailListFilters } from '@/lib/query-keys'
import { z } from '@/lib/zod'
import {
  followupDaysAtom,
  followupDueBy,
  type InboxSection,
  inboxSectionAtom,
  sectionedInboxAtom,
} from '@/store/inbox-sections'
import { conversationKeys } from '@/store/query-keys-v21'
import { hideSuspectedFraudAtom } from '@/store/settings'
import {
  activeLabelAtom,
  activeListAtom,
  categoryFilterAtom,
  searchQueryAtom,
  selectedAccountsAtom,
  selectedDomainsAtom,
} from '@/store/ui'
import { wireFetch } from '@/wire/client'

/** Same 200 ms search debounce as the list's input. */
const SEARCH_DEBOUNCE_MS = 200

export function useCurrentMailFilters(): MailListFilters | null {
  const list = useAtomValue(activeListAtom)
  const activeLabel = useAtomValue(activeLabelAtom)
  const categoryFilter = useAtomValue(categoryFilterAtom)
  const selectedDomains = useAtomValue(selectedDomainsAtom)
  const selectedAccounts = useAtomValue(selectedAccountsAtom)
  const searchQuery = useAtomValue(searchQueryAtom)
  const sectioned = useAtomValue(sectionedInboxAtom)
  const inboxSection = useAtomValue(inboxSectionAtom)
  const followupDays = useAtomValue(followupDaysAtom)
  const hideSuspectedFraud = useAtomValue(hideSuspectedFraudAtom)
  const debouncedSearch = useDebouncedValue(searchQuery, SEARCH_DEBOUNCE_MS)

  return useMemo<MailListFilters | null>(() => {
    const axes = sectionAxes(labelAxes(threadAxesOf(list), list, activeLabel), list, {
      days: followupDays,
      on: sectioned,
      section: inboxSection,
    })
    if (!axes) return null
    // The list fixes its axes; category, section, search and domains are
    // refinements stacked on top of whichever list is showing.
    return {
      ...axes,
      // `null` passes through as `null`: no filter. It is deliberately
      // not collapsed with `[]`, which is somebody who unticked every
      // account and must not be served the unfiltered list.
      accounts: selectedAccounts,
      category: categoryFilter,
      domains: selectedDomains.length > 0 ? selectedDomains : undefined,
      // The setting says whether to hide them; the request asks for
      // them. Two ideas, and the negation is where they meet — a
      // reader who has switched hiding off is asking to be shown what
      // is held, marked, in place.
      includeQuarantined: !hideSuspectedFraud,
      query: debouncedSearch || undefined,
    }
  }, [
    list,
    activeLabel,
    categoryFilter,
    selectedAccounts,
    selectedDomains,
    debouncedSearch,
    hideSuspectedFraud,
    sectioned,
    inboxSection,
    followupDays,
  ])
}

/**
 * Unread conversations across the mailbox, as the server counts them.
 *
 * The badge renders in the app sidebar and the mobile shell, which are
 * up outside the mail screen entirely. It used to add up the rows of
 * the Unread list this client had loaded, so past the first page it
 * counted short. The server counts the same buckets the Unread list
 * shows (`count_flag_non_junk`). Keyed under the conversation lists, so
 * whatever refreshes the lists after a read or a move refreshes this.
 */
export function useCurrentUnreadCount(): number {
  return useUnreadCounts().count
}

/**
 * The unread conversations outside Junk (`count`, what the Unread list
 * shows), and the share of them each of the Inbox and Notifications &
 * Promotions lists.
 */
export function useUnreadCounts(): UnreadCounts {
  const { data } = useQuery({
    queryKey: [...conversationKeys.lists(), 'unseen-count'],
    queryFn: ({ signal }) =>
      wireFetch(unseenCountSchema, { path: '/conversations/unseen-count', signal }),
  })
  return data ?? NO_UNREAD
}

// Backend: crates/webapi/src/handlers/conversations.rs — fn get_unseen_count
const unseenCountSchema = z.object({ count: z.number(), inbox: z.number(), np: z.number() })

export type UnreadCounts = z.infer<typeof unseenCountSchema>

const NO_UNREAD: UnreadCounts = { count: 0, inbox: 0, np: 0 }

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
