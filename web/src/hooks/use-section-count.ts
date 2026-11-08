import type { InboxSection } from '@/store/inbox-sections'

import { useAtomValue } from 'jotai'

import { useConversationsQuery } from '@/hooks/use-mail-queries'
import { LIST_PAGE_SIZE } from '@/lib/list-axes'
import { mailFiltersOf } from '@/lib/mail-filters'
import { followupDaysAtom } from '@/store/inbox-sections'
import { hideSuspectedFraudAtom } from '@/store/settings'
import {
  activeListAtom,
  categoryFilterAtom,
  selectedAccountsAtom,
  selectedDomainsAtom,
} from '@/store/ui'

/** How many conversations a section holds, as its chip shows it. */
export type SectionCount = { full: boolean; n: number }

/**
 * How many conversations `section` of the inbox holds: the first page of
 * the section's own list, the request its chip makes when chosen and under
 * the same cache key, so choosing it shows what was counted and a mail
 * invalidation refreshes both. Null until that page has come back.
 */
export function useSectionCount(section: InboxSection, enabled: boolean): null | SectionCount {
  const list = useAtomValue(activeListAtom)
  const category = useAtomValue(categoryFilterAtom)
  const accounts = useAtomValue(selectedAccountsAtom)
  const domains = useAtomValue(selectedDomainsAtom)
  const followupDays = useAtomValue(followupDaysAtom)
  const hideSuspectedFraud = useAtomValue(hideSuspectedFraudAtom)
  // a category chosen in another list is dropped when the chip opens the inbox
  let inboxCategory: null | string = null
  if (list === 'inbox') inboxCategory = category
  const filters = mailFiltersOf({
    accounts,
    category: inboxCategory,
    domains,
    followupDays,
    hideSuspectedFraud,
    inboxSection: section,
    label: null,
    list: 'inbox',
    search: '',
    sectioned: true,
  })
  // the inbox is always served by `/conversations`
  const { data } = useConversationsQuery(filters as NonNullable<typeof filters>, enabled)
  const first = data?.pages[0]
  if (!first) return null
  return { full: first.length >= LIST_PAGE_SIZE, n: first.length }
}
