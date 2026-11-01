/**
 * Audit actions as the backend writes them: `<resource>.<verb>`, from
 * webapi's recorder (`handlers/audit.rs` callers) and fastcore's system
 * events (`mail.bounce`, `mail.send_failed`). A filter value is a prefix
 * that includes the dot, so `group.` does not also match `email_group.`
 */
import type { Key } from '@/i18n'

export const ACTION_GROUPS: { label: Key; prefixes: string[]; value: string }[] = [
  { label: 'admin.audit.group.all', prefixes: [], value: '' },
  { label: 'admin.audit.group.auth', prefixes: ['auth.'], value: 'auth' },
  { label: 'admin.domains.title', prefixes: ['domain.'], value: 'domain' },
  { label: 'admin.accounts.title', prefixes: ['account.', 'overrides.'], value: 'account' },
  { label: 'admin.aliases.title', prefixes: ['alias.'], value: 'alias' },
  { label: 'admin.permissionGroups', prefixes: ['group.'], value: 'group' },
  { label: 'admin.emailGroups.title', prefixes: ['email_group.'], value: 'email_group' },
  { label: 'admin.audit.group.webhooks', prefixes: ['webhook.'], value: 'webhook' },
  { label: 'admin.audit.group.sieve', prefixes: ['sieve.'], value: 'sieve' },
  { label: 'admin.audit.group.mail', prefixes: ['mail.'], value: 'mail' },
  { label: 'admin.audit.group.suppressions', prefixes: ['suppressions.'], value: 'suppressions' },
  { label: 'admin.audit.group.cache', prefixes: ['cache.'], value: 'cache' },
  { label: 'admin.audit.group.systemConfig', prefixes: ['system_config.'], value: 'system_config' },
  { label: 'admin.audit.group.greylist', prefixes: ['greylist.'], value: 'greylist' },
  { label: 'admin.audit.group.mock', prefixes: ['mock.'], value: 'mock' },
  {
    label: 'admin.audit.group.userContent',
    prefixes: ['draft.', 'signature.', 'template.'],
    value: 'user_content',
  },
  { label: 'admin.audit.group.reads', prefixes: ['audit.'], value: 'audit' },
]

export function actionColor(action: string): string {
  if (action === 'mail.send_failed' || action === 'mail.bounce') return 'text-danger'
  if (action.startsWith('auth.login')) return 'text-success'
  if (action.startsWith('audit.')) return 'text-warning'
  return 'text-fg'
}

export function matchesActionFilter(action: string, filter: string): boolean {
  if (!filter) return true
  const group = ACTION_GROUPS.find((g) => g.value === filter)
  if (!group) return false
  return group.prefixes.some((p) => action.startsWith(p))
}
