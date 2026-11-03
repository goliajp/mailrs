import type { AuthInfo } from '@/store/auth'

/**
 * What each admin page needs, mirroring the server's route table
 * (`route_permissions.rs`): the permission its main route is guarded by.
 * The server still refuses whatever is not allowed; this only hides the
 * pages a person could not use.
 */
type Need = 'any-admin' | 'super' | `admin.${string}`

export const PAGE_NEED: Record<string, Need> = {
  '/admin/accounts': 'admin.accounts',
  '/admin/aliases': 'admin.aliases',
  '/admin/audit-log': 'super',
  '/admin/dmarc': 'admin.domains',
  '/admin/domains': 'admin.domains',
  '/admin/email-groups': 'admin.accounts',
  '/admin/greylist': 'admin.greylist',
  '/admin/groups': 'admin.groups',
  '/admin/mail-audit': 'super',
  '/admin/mock': 'admin.system_config',
  '/admin/overview': 'any-admin',
  '/admin/password-clear': 'super',
  '/admin/protocol-sign-ins': 'super',
  '/admin/queues': 'super',
  '/admin/reports': 'super',
  '/admin/screening': 'super',
  '/admin/sign-ins': 'super',
  '/admin/system-config': 'admin.system_config',
}

type Who = Pick<AuthInfo, 'is_admin' | 'is_super' | 'permissions'>

/** Whether this person can use the admin page at `path`; unknown pages are super-only, as on the server. */
export function canOpenAdminPage(who: null | undefined | Who, path: string): boolean {
  if (!who) return false
  if (who.is_super) return true
  const need = PAGE_NEED[path] ?? 'super'
  if (need === 'super') return false
  if (need === 'any-admin') return who.is_admin
  return who.permissions.includes(need)
}

/** Whether the admin entry belongs in the navigation at all. */
export function showsAdmin(who: null | undefined | Who): boolean {
  return Boolean(who?.is_admin || who?.is_super)
}
