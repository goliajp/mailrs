// Backend: crates/webapi/src/handlers/groups.rs — fn list_email_groups
export type EmailGroupInfo = {
  address: string
  created_at: number
  description: string
  domain: string
  id: number
  name: string
}
