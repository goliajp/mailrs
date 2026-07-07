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

import { useAtomValue } from 'jotai'
import { useMemo } from 'react'

import { useDebouncedValue } from '@/hooks/use-debounced-value'
import { useFlatConversations } from '@/hooks/use-flat-conversations'
import { type MailListFilters } from '@/lib/query-keys'
import {
  categoryFilterAtom,
  folderAtom,
  importanceSectionAtom,
  quickFilterAtom,
  searchQueryAtom,
  selectedDomainsAtom,
  showArchivedAtom,
} from '@/store/chat'

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
