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
  templates: () => [...mailKeys.all(), 'templates'] as const,
  thread: (threadId: null | string) => [...mailKeys.all(), 'thread', threadId ?? ''] as const,
}

export const adminKeys = {
  accounts: () => [...adminKeys.all(), 'accounts'] as const,
  aliases: () => [...adminKeys.all(), 'aliases'] as const,
  all: () => ['admin'] as const,
  apps: () => [...adminKeys.all(), 'apps'] as const,
  auditLog: () => [...adminKeys.all(), 'audit-log'] as const,
  domains: () => [...adminKeys.all(), 'domains'] as const,
  emailGroupMembers: (id: number) => [...adminKeys.all(), 'email-group-members', id] as const,
  emailGroups: () => [...adminKeys.all(), 'email-groups'] as const,
  greylistLocal: () => [...adminKeys.all(), 'greylist-local'] as const,
  greylistLocalHealth: () => [...adminKeys.all(), 'greylist-local-health'] as const,
  groupMembers: (id: number) => [...adminKeys.all(), 'group-members', id] as const,
  groupPermissions: (id: number) => [...adminKeys.all(), 'group-permissions', id] as const,
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
  agentKeys: () => [...settingsKeys.all(), 'agent-keys'] as const,
  all: () => ['settings'] as const,
  calendarFeeds: () => [...settingsKeys.all(), 'calendar-feeds'] as const,
  encryptionKeysStatus: () => [...settingsKeys.all(), 'encryption-keys-status'] as const,
  recoveryEmail: () => [...settingsKeys.all(), 'recovery-email'] as const,
  signatures: () => [...settingsKeys.all(), 'signatures'] as const,
  totpStatus: () => [...settingsKeys.all(), 'totp-status'] as const,
  webhooks: () => [...settingsKeys.all(), 'webhooks'] as const,
}

export const dashboardKeys = {
  all: () => ['dashboard'] as const,
  conversations: () => [...dashboardKeys.all(), 'conversations'] as const,
  folders: () => [...dashboardKeys.all(), 'folders'] as const,
  stats: () => [...dashboardKeys.all(), 'stats'] as const,
}

export const calendarKeys = {
  all: () => ['calendar'] as const,
  conflicts: (startIso: string, endIso: string, excludeUid: string) =>
    [...calendarKeys.all(), 'conflicts', startIso, endIso, excludeUid] as const,
}

export const messageKeys = {
  all: () => ['message'] as const,
  detail: (uid: number) => [...messageKeys.all(), 'detail', uid] as const,
}

export const contactsKeys = {
  all: () => ['contacts'] as const,
  search: (q: string) => [...contactsKeys.all(), 'search', q] as const,
}
