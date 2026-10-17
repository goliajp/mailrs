/**
 * Audit actions as the backend writes them: `<resource>.<verb>`, from
 * webapi's recorder (`handlers/audit.rs` callers) and fastcore's system
 * events (`mail.bounce`, `mail.send_failed`). A filter value is a prefix
 * that includes the dot, so `group.` does not also match `email_group.`
 */
export const ACTION_GROUPS: { label: string; prefixes: string[]; value: string }[] = [
  { label: 'all', prefixes: [], value: '' },
  { label: 'auth (login / linked identities)', prefixes: ['auth.'], value: 'auth' },
  { label: 'domain', prefixes: ['domain.'], value: 'domain' },
  { label: 'account', prefixes: ['account.', 'overrides.'], value: 'account' },
  { label: 'alias', prefixes: ['alias.'], value: 'alias' },
  { label: 'group', prefixes: ['group.'], value: 'group' },
  { label: 'email group', prefixes: ['email_group.'], value: 'email_group' },
  { label: 'app', prefixes: ['app.'], value: 'app' },
  { label: 'webhook', prefixes: ['webhook.'], value: 'webhook' },
  { label: 'sieve', prefixes: ['sieve.'], value: 'sieve' },
  { label: 'mail delivery (bounce / send failed)', prefixes: ['mail.'], value: 'mail' },
  { label: 'suppressions', prefixes: ['suppressions.'], value: 'suppressions' },
  { label: 'cache', prefixes: ['cache.'], value: 'cache' },
  { label: 'system config', prefixes: ['system_config.'], value: 'system_config' },
  { label: 'greylist', prefixes: ['greylist.'], value: 'greylist' },
  {
    label: 'drafts / signatures / templates',
    prefixes: ['draft.', 'signature.', 'template.'],
    value: 'user_content',
  },
  { label: "audit read (someone else's mail)", prefixes: ['audit.'], value: 'audit' },
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
