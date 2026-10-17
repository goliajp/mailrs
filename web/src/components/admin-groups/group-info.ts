// Backend: crates/webapi/src/handlers/groups.rs — fn list_groups. Rows
// written before 2026-10-17 carry neither `domain` nor `is_builtin`.
export type GroupInfo = {
  description: string
  domain?: null | string
  id: number
  is_builtin?: boolean
  name: string
}

export function domainLabel(domain: GroupInfo['domain']): string {
  if (domain === undefined) return '—'
  if (domain === null) return 'Global'
  return domain
}
