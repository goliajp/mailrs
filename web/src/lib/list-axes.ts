import type { MailListFilters } from '@/lib/query-keys'

import { conversationKeys } from '@/store/query-keys-v21'

/** Rows asked for per page of a list, and per search. */
export const LIST_PAGE_SIZE = 50

/** The cache line a list's pages live in, for the list and its route preload alike. */
export function conversationListKey(filters: MailListFilters) {
  return conversationKeys.infinite({
    accounts: filters.accounts,
    archived: filters.archived,
    assignedToMe: filters.assignedToMe,
    category: filters.category as never,
    domains: filters.domains,
    folder: filters.folder as never,
    followupDueBy: filters.followupDueBy,
    includeQuarantined: filters.includeQuarantined,
    label: filters.label,
    pinned: filters.pinned,
    quarantined: filters.quarantined,
    // `query` MUST be in the key: listPath() switches to the
    // /conversations/search endpoint when filters.query is set, but
    // without query in the key a search reuses the non-search inbox
    // cache and never refetches — search silently shows the inbox.
    query: filters.query,
    section: filters.section,
    snoozed: filters.snoozed,
    starred: filters.starred,
    trashed: filters.trashed,
    unread: filters.unread,
  })
}

/**
 * The list's own parameters, the ones `/api/conversations` and its
 * `/search` both read.
 *
 * One builder for the two, because the list and the search inside it
 * must scope to the same thing. Search used to have its own copy that
 * left out the Trash, labels, Pinned, Snoozed and the sections, so a
 * search typed into any of them answered from every list. Booleans are
 * `true`: the server reads `1` as no boolean at all.
 */
export function listAxes(filters: MailListFilters): string {
  let p = ''
  if (filters.trashed) p += '&trashed=true'
  if (filters.label) p += `&label=${encodeURIComponent(filters.label)}`
  if (filters.category) p += `&category=${encodeURIComponent(filters.category)}`
  if (filters.domains && filters.domains.length > 0) {
    p += `&domains=${encodeURIComponent(filters.domains.join(','))}`
  }
  if (filters.archived) p += '&archived=true'
  if (filters.includeQuarantined) p += '&include_quarantined=true'
  if (filters.folder) p += `&folder=${encodeURIComponent(filters.folder)}`
  if (filters.unread) p += '&unread=true'
  if (filters.starred) p += '&starred=true'
  if (filters.pinned) p += '&pinned=true'
  if (filters.snoozed) p += '&snoozed=true'
  if (filters.section) p += `&section=${encodeURIComponent(filters.section)}`
  if (filters.followupDueBy) p += `&followup_due_by=${filters.followupDueBy}`
  if (filters.assignedToMe) p += '&assigned_to_me=true'
  p += accountsParam(filters)
  return p
}

/**
 * The API path for one page of a conversation list, or for a search
 * inside it.
 *
 * Here and not in the list's hook so the route preload asks for the same
 * page without importing the hook. The preload used to build its own path
 * from URL parameters nothing writes, so it fetched a page no list read.
 */
export function listPath(filters: MailListFilters, cursor?: string): string {
  if (filters.query) {
    const p = `/conversations/search?q=${encodeURIComponent(filters.query)}&limit=${LIST_PAGE_SIZE}`
    // the review list has no list parameters: it is its own endpoint
    if (filters.quarantined) return `${p}&quarantined=true`
    return p + listAxes(filters)
  }
  // The review list is its own endpoint — held mail is not a
  // predicate over the ordinary lists, it is the thing they exclude.
  // Paged like the rest: the client appends pages, so a route that
  // ignored the cursor would show every held conversation twice.
  if (filters.quarantined) {
    let p = `/quarantine?limit=${LIST_PAGE_SIZE}`
    if (cursor) p += `&cursor=${encodeURIComponent(cursor)}`
    return p
  }
  let p = `/conversations?limit=${LIST_PAGE_SIZE}`
  if (cursor) p += `&cursor=${encodeURIComponent(cursor)}`
  return p + listAxes(filters)
}

/**
 * The `accounts=` parameter, or nothing.
 *
 * Sent whenever the filter is narrowed — including when it is narrowed
 * to nothing, which is `accounts=` with an empty value. The server
 * reads an absent parameter as every account and an empty one as none,
 * so the two cannot be collapsed here.
 */
function accountsParam(filters: MailListFilters): string {
  if (!filters.accounts) return ''
  return `&accounts=${encodeURIComponent(filters.accounts.join(','))}`
}
