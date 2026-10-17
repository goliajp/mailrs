/**
 * Audit actions as the backend writes them: `<resource>.<verb>`, from
 * webapi's recorder (`handlers/audit.rs` callers) and fastcore's system
 * events (`mail.bounce`, `mail.send_failed`). A filter value is a prefix
 * that includes the dot, so `group.` does not also match `email_group.`
 */
export const ACTION_GROUPS: { label: string; prefixes: string[]; value: string }[] = [
  { label: 'All actions', prefixes: [], value: '' },
  { label: 'Sign-ins and linked identities', prefixes: ['auth.'], value: 'auth' },
  { label: 'Domains', prefixes: ['domain.'], value: 'domain' },
  { label: 'Accounts', prefixes: ['account.', 'overrides.'], value: 'account' },
  { label: 'Aliases', prefixes: ['alias.'], value: 'alias' },
  { label: 'Permission groups', prefixes: ['group.'], value: 'group' },
  { label: 'Email groups', prefixes: ['email_group.'], value: 'email_group' },
  { label: 'Apps', prefixes: ['app.'], value: 'app' },
  { label: 'Webhooks', prefixes: ['webhook.'], value: 'webhook' },
  { label: 'Sieve filters', prefixes: ['sieve.'], value: 'sieve' },
  { label: 'Delivery failures and bounces', prefixes: ['mail.'], value: 'mail' },
  { label: 'Suppressions', prefixes: ['suppressions.'], value: 'suppressions' },
  { label: 'Cache', prefixes: ['cache.'], value: 'cache' },
  { label: 'System config', prefixes: ['system_config.'], value: 'system_config' },
  { label: 'Greylist', prefixes: ['greylist.'], value: 'greylist' },
  {
    label: 'Drafts, signatures and templates',
    prefixes: ['draft.', 'signature.', 'template.'],
    value: 'user_content',
  },
  { label: "Reads of other people's mail", prefixes: ['audit.'], value: 'audit' },
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
