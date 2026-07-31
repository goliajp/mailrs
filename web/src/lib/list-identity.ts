import type { MailListFilters } from '@/lib/query-keys'

/**
 * Which list is on screen, as one string.
 *
 * `MailListFilters` is already the canonical answer to that question — it is
 * what the query is keyed by — so the scroll position and the selection reset
 * off the same definition the data does. A second notion of "which list"
 * defined next to the component is how the two would drift.
 *
 * Its own module because the list keeps a scroll position per list and clears
 * the selection when this value changes, and both need to agree with each
 * other and with the query cache.
 */
export function listIdentity(f: MailListFilters): string {
  return JSON.stringify([
    f.folder,
    f.category,
    f.section,
    f.starred,
    f.unread,
    f.archived,
    f.query,
    f.domains,
  ])
}
