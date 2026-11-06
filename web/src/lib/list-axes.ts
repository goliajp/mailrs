import type { MailListFilters } from '@/lib/query-keys'

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
