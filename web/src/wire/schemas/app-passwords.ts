import { z } from '@/lib/zod'

// Backend: crates/auth-guard/src/app_passwords.rs — `Info`
export const appPasswordSchema = z.object({
  created_at: z.number(),
  id: z.string(),
  last_used_at: z.number().nullable(),
  name: z.string(),
})

export type WireAppPassword = z.infer<typeof appPasswordSchema>

export const appPasswordsSchema = z.array(appPasswordSchema)

// Backend: crates/webapi/src/handlers/app_passwords.rs — fn create
export const madeAppPasswordSchema = z.object({
  id: z.string(),
  name: z.string(),
  password: z.string(),
})

// Backend: crates/webapi/src/handlers/app_passwords.rs — fn revoke_all
export const revokedSchema = z.object({ revoked: z.number() })
