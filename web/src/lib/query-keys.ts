// Centralized query key factory.
//
// All cache reads and invalidations route through these helpers — never
// hand-write a query key inline. The factory exists so that:
//   1. Invalidation can be done at any prefix (e.g. `mailKeys.all()` busts
//      every mail query at once on logout, `mailKeys.conversations()` busts
//      every conversation-list variant when a NewMessage arrives).
//   2. Filter shape changes show up as type errors at the call site instead
//      of silently mismatching keys.
//   3. Future per-namespace gcTime / staleTime overrides are localized.

export type MailListFilters = {
  archived?: boolean
  category?: null | string
  domains?: string[]
  folder?: null | string
  query?: string
  section?: null | string
  starred?: boolean
  unread?: boolean
}

// Normalize filters into a stable, JSON-serializable shape so equivalent
// filters produce the same query key regardless of property-set order or
// undefined-vs-missing distinctions. RQ compares keys via deep equality on
// the array, so consistency matters.
function normalizeFilters(f: MailListFilters): Record<string, boolean | number | string> {
  const out: Record<string, boolean | number | string> = {}
  if (f.archived) out.archived = 1
  if (f.category) out.category = f.category
  if (f.domains && f.domains.length > 0) out.domains = [...f.domains].sort().join(',')
  if (f.folder) out.folder = f.folder
  if (f.query) out.query = f.query
  if (f.section) out.section = f.section
  if (f.starred) out.starred = 1
  if (f.unread) out.unread = 1
  return out
}

export const mailKeys = {
  actionCount: (domains: string[]) =>
    [...mailKeys.all(), 'action-count', [...domains].sort().join(',')] as const,
  all: () => ['mail'] as const,
  categories: (domains: string[]) =>
    [...mailKeys.all(), 'categories', [...domains].sort().join(',')] as const,
  conversations: (filters?: MailListFilters) =>
    [...mailKeys.all(), 'conversations', filters ? normalizeFilters(filters) : {}] as const,
  search: (q: string, filters?: MailListFilters) =>
    [...mailKeys.all(), 'search', q, filters ? normalizeFilters(filters) : {}] as const,
  thread: (threadId: null | string) => [...mailKeys.all(), 'thread', threadId ?? ''] as const,
}

export const adminKeys = {
  accounts: () => [...adminKeys.all(), 'accounts'] as const,
  aliases: () => [...adminKeys.all(), 'aliases'] as const,
  all: () => ['admin'] as const,
  apps: () => [...adminKeys.all(), 'apps'] as const,
  auditLog: () => [...adminKeys.all(), 'audit-log'] as const,
  domains: () => [...adminKeys.all(), 'domains'] as const,
  emailGroups: () => [...adminKeys.all(), 'email-groups'] as const,
  groups: () => [...adminKeys.all(), 'groups'] as const,
  mailAuditAccounts: () => [...adminKeys.all(), 'mail-audit-accounts'] as const,
  mailAuditConversations: (address: string) =>
    [...adminKeys.all(), 'mail-audit-conversations', address] as const,
  mailAuditThread: (address: string, threadId: string) =>
    [...adminKeys.all(), 'mail-audit-thread', address, threadId] as const,
  overviewAuditLog: () => [...adminKeys.all(), 'overview-audit-log'] as const,
  overviewHealth: () => [...adminKeys.all(), 'overview-health'] as const,
  overviewSmtp: () => [...adminKeys.all(), 'overview-smtp'] as const,
  overviewStatus: () => [...adminKeys.all(), 'overview-status'] as const,
  permissions: () => [...adminKeys.all(), 'permissions'] as const,
  queues: () => [...adminKeys.all(), 'queues'] as const,
  systemConfig: () => [...adminKeys.all(), 'system-config'] as const,
}

export const settingsKeys = {
  all: () => ['settings'] as const,
  signatures: () => [...settingsKeys.all(), 'signatures'] as const,
}
