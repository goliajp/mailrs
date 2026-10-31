import { z } from '@/lib/zod'

const policySchema = z.enum(['inherit', 'allow', 'app-only'])

export type WirePolicy = z.infer<typeof policySchema>

// Backend: crates/auth-guard/src/protocol_usage.rs — `Usage`
export const usageSchema = z.object({
  account: z.string(),
  app_password_last: z.number().nullable(),
  app_password_logins: z.number(),
  channel: z.string(),
  password_last: z.number().nullable(),
  password_logins: z.number(),
})

export type WireUsage = z.infer<typeof usageSchema>

// Backend: crates/webapi/src/handlers/app_passwords.rs — fn usage
export const protocolLoginsSchema = z.object({
  days: z.number(),
  default: z.enum(['allow', 'app-only']),
  items: z.array(usageSchema),
  policies: z.record(z.string(), policySchema),
})
