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

import { useAtomValue } from 'jotai'
import { useMemo } from 'react'

import { useDebouncedValue } from '@/hooks/use-debounced-value'
import { useFlatConversations } from '@/hooks/use-flat-conversations'
import { MAIL_LISTS, threadAxesOf } from '@/lib/mail-lists'
import { type MailListFilters } from '@/lib/query-keys'
import {
  activeListAtom,
  categoryFilterAtom,
  importanceSectionAtom,
  searchQueryAtom,
  selectedDomainsAtom,
} from '@/store/ui'

/** Same 200 ms search debounce as the list's input. */
const SEARCH_DEBOUNCE_MS = 200

export function useCurrentMailFilters(): MailListFilters | null {
  const list = useAtomValue(activeListAtom)
  const categoryFilter = useAtomValue(categoryFilterAtom)
  const selectedDomains = useAtomValue(selectedDomainsAtom)
  const importanceSection = useAtomValue(importanceSectionAtom)
  const searchQuery = useAtomValue(searchQueryAtom)
  const debouncedSearch = useDebouncedValue(searchQuery, SEARCH_DEBOUNCE_MS)

  return useMemo<MailListFilters | null>(() => {
    const axes = threadAxesOf(list)
    if (!axes) return null
    // The list fixes its axes; category, section, search and domains are
    // refinements stacked on top of whichever list is showing.
    return {
      ...axes,
      category: categoryFilter,
      domains: selectedDomains.length > 0 ? selectedDomains : undefined,
      query: debouncedSearch || undefined,
      section: importanceSection,
    }
  }, [list, categoryFilter, selectedDomains, debouncedSearch, importanceSection])
}

/**
 * Unread across the mailbox, not across the list on screen.
 *
 * The badge renders in the app sidebar and the mobile shell, which are
 * up outside the mail screen entirely — so "whatever list is showing"
 * was not a question those had an answer to. It reads the Unread list's
 * own axes, which is the set that tab lists and the set the server's
 * `count_flag_non_junk` counts.
 */
export function useCurrentUnreadCount(): number {
  const selectedDomains = useAtomValue(selectedDomainsAtom)
  const filters = useMemo<MailListFilters>(() => {
    const source = MAIL_LISTS.unread.source
    return {
      ...(source.kind === 'threads' ? source.filters : {}),
      domains: selectedDomains.length > 0 ? selectedDomains : undefined,
    }
  }, [selectedDomains])
  const { conversations } = useFlatConversations(filters)
  return useMemo(() => conversations.reduce((sum, c) => sum + c.unread_count, 0), [conversations])
}
