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
import { z } from 'zod'

import { useDebouncedValue } from '@/hooks/use-debounced-value'
import { threadAxesOf } from '@/lib/mail-lists'
import { type MailListFilters } from '@/lib/query-keys'
import { conversationKeys } from '@/store/query-keys-v21'
import { hideSuspectedFraudAtom } from '@/store/settings'
import {
  activeListAtom,
  categoryFilterAtom,
  importanceSectionAtom,
  searchQueryAtom,
  selectedAccountsAtom,
  selectedDomainsAtom,
} from '@/store/ui'
import { wireFetch } from '@/wire/client'

/** Same 200 ms search debounce as the list's input. */
const SEARCH_DEBOUNCE_MS = 200

export function useCurrentMailFilters(): MailListFilters | null {
  const list = useAtomValue(activeListAtom)
  const categoryFilter = useAtomValue(categoryFilterAtom)
  const selectedDomains = useAtomValue(selectedDomainsAtom)
  const selectedAccounts = useAtomValue(selectedAccountsAtom)
  const importanceSection = useAtomValue(importanceSectionAtom)
  const searchQuery = useAtomValue(searchQueryAtom)
  const hideSuspectedFraud = useAtomValue(hideSuspectedFraudAtom)
  const debouncedSearch = useDebouncedValue(searchQuery, SEARCH_DEBOUNCE_MS)

  return useMemo<MailListFilters | null>(() => {
    const axes = threadAxesOf(list)
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
      section: importanceSection,
    }
  }, [
    list,
    categoryFilter,
    selectedAccounts,
    selectedDomains,
    debouncedSearch,
    importanceSection,
    hideSuspectedFraud,
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
  const { data } = useQuery({
    queryKey: [...conversationKeys.lists(), 'unseen-count'],
    queryFn: ({ signal }) =>
      wireFetch(unseenCountSchema, { path: '/conversations/unseen-count', signal }),
  })
  return data?.count ?? 0
}

const unseenCountSchema = z.object({ count: z.number() })
