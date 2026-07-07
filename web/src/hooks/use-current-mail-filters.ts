/**
 * `useCurrentMailFilters` — the ONE way for any mail-facing component
 * to know what filter is currently active.
 *
 * v2.1 phase-5b introduces this hook so `conversation-list.tsx`,
 * `mobile-mail.tsx`, and eventually `thread-view.tsx` can each read
 * from `useFlatConversations(filters)` without duplicating the
 * atom-composition logic that today lives inline in `chat.tsx`.
 *
 * The hook mirrors `chat.tsx`'s existing `filters` memo — same
 * fields, same defaults, same debounced-search treatment. Keep them
 * in lock-step: any new filter axis added here must also land on
 * chat.tsx (until Phase 5d unifies them by deleting the atom-based
 * mirror in chat.tsx).
 */

import type { ThreadMessage } from '@/lib/types'

import { useAtomValue } from 'jotai'
import { useMemo } from 'react'

import { useDebouncedValue } from '@/hooks/use-debounced-value'
import { useFlatConversations } from '@/hooks/use-flat-conversations'
import { useThreadQuery } from '@/hooks/use-mail-queries'
import { type MailListFilters } from '@/lib/query-keys'
import {
  categoryFilterAtom,
  folderAtom,
  importanceSectionAtom,
  quickFilterAtom,
  searchQueryAtom,
  selectedDomainsAtom,
  selectedThreadIdAtom,
  showArchivedAtom,
} from '@/store/ui'

/**
 * Stable empty-array reference so `?? []` doesn't manufacture a fresh
 * array reference on every render (which would defeat React.memo on
 * downstream children).
 */
const EMPTY_MESSAGES: readonly ThreadMessage[] = []

/** Same 200 ms search debounce as chat.tsx. */
const SEARCH_DEBOUNCE_MS = 200

export function useCurrentMailFilters(): MailListFilters {
  const folder = useAtomValue(folderAtom)
  const categoryFilter = useAtomValue(categoryFilterAtom)
  const selectedDomains = useAtomValue(selectedDomainsAtom)
  const quickFilter = useAtomValue(quickFilterAtom)
  const importanceSection = useAtomValue(importanceSectionAtom)
  const showArchived = useAtomValue(showArchivedAtom)
  const searchQuery = useAtomValue(searchQueryAtom)
  const debouncedSearch = useDebouncedValue(searchQuery, SEARCH_DEBOUNCE_MS)

  return useMemo<MailListFilters>(
    () => ({
      archived: showArchived,
      category: categoryFilter,
      domains: selectedDomains.length > 0 ? selectedDomains : undefined,
      folder,
      query: debouncedSearch || undefined,
      section: importanceSection,
      starred: quickFilter === 'starred',
      unread: quickFilter === 'unread',
    }),
    [
      showArchived,
      categoryFilter,
      selectedDomains,
      folder,
      debouncedSearch,
      importanceSection,
      quickFilter,
    ]
  )
}

/**
 * The messages of the currently-selected thread, as the mail list
 * defines "current." Wraps `useThreadQuery` with the same
 * selectedThreadId + selectedDomains atoms every reader would
 * otherwise pluck separately.
 *
 * v2.1 phase-5d finale: replaces `useAtomValue(threadMessagesAtom)`
 * for read-only callers (`reply-box`, `mobile-mail` views). RQ dedupes
 * the underlying query, so N components subscribing here still results
 * in one fetch, one cache line, one identity-stable value.
 */
export function useCurrentThreadMessages() {
  const selectedThreadId = useAtomValue(selectedThreadIdAtom)
  const selectedDomains = useAtomValue(selectedDomainsAtom)
  const { data } = useThreadQuery(selectedThreadId, selectedDomains)
  return data ?? EMPTY_MESSAGES
}

/**
 * Sum unread_count over the currently-visible conversation list.
 *
 * v2.1 phase-5d — this replaces the derived `unreadCountAtom`, which
 * summed over the atom-shadowed conversation list. Same semantic:
 * whatever filter the mail list is currently showing is what this
 * badge reflects. The AppSidebar / mobile-shell / document-title
 * effect all subscribe to it.
 */
export function useCurrentUnreadCount(): number {
  const filters = useCurrentMailFilters()
  const { conversations } = useFlatConversations(filters)
  return useMemo(() => conversations.reduce((sum, c) => sum + c.unread_count, 0), [conversations])
}
