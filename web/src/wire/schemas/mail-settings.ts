import { z } from '@/lib/zod'

// Backend: crates/webapi/src/handlers/mail_settings.rs — fn get_vacation / fn put_vacation
// (the type is crates/core-sidestate/src/families/vacation.rs `Vacation`)
export const vacationSchema = z.object({
  body: z.string(),
  days: z.number(),
  enabled: z.boolean(),
  end: z.number().nullable(),
  start: z.number().nullable(),
  subject: z.string(),
  version: z.number(),
})

export type WireVacation = z.infer<typeof vacationSchema>

// Backend: crates/webapi/src/handlers/mail_settings.rs — fn get_forwarding (`ForwardingView`)
export const forwardingSchema = z.object({
  address: z.string(),
  keep_copy: z.boolean(),
  pending_address: z.string(),
})

export type WireForwarding = z.infer<typeof forwardingSchema>

// Backend: crates/webapi/src/handlers/mail_settings.rs — fn verify_forwarding
export const forwardingVerifiedSchema = z.object({ address: z.string() })
