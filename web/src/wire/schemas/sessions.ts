import { z } from '@/lib/zod'

// Backend: crates/webapi/src/handlers/auth/sessions.rs — fn list (`SessionView`)
export const sessionSchema = z.object({
  created_at: z.number(),
  current: z.boolean(),
  id: z.string(),
  ip: z.string(),
  last_seen: z.number(),
  user_agent: z.string(),
})

export type WireSession = z.infer<typeof sessionSchema>

export const sessionsSchema = z.array(sessionSchema)

// Backend: crates/webapi/src/handlers/auth/sessions.rs — fn end_others
export const endedSchema = z.object({ ended: z.number() })
