// Backend: crates/webapi/src/handlers/groups.rs — fn list_groups. Rows
// written before 2026-10-17 carry no `is_builtin`. `domain` is always null
// now: permissions are not scoped to a domain.
export type GroupInfo = {
  description: string
  id: number
  is_builtin?: boolean
  name: string
}
