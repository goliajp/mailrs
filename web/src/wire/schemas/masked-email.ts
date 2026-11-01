import { z } from '@/lib/zod'

// Backend: crates/core-sidestate/src/families/masked_email/mod.rs Mask,
// as crates/webapi/src/handlers/masked_email.rs returns it
export const maskedEmailSchema = z.object({
  createdAt: z.number(),
  createdBy: z.string().default(''),
  description: z.string().default(''),
  email: z.string(),
  forDomain: z.string().default(''),
  id: z.string(),
  lastMessageAt: z.number().nullable().default(null),
  owner: z.string(),
  state: z.enum(['pending', 'enabled', 'disabled', 'deleted']),
  url: z.string().nullable().default(null),
})

export const maskedEmailListSchema = z.array(maskedEmailSchema)

export type WireMaskedEmail = z.infer<typeof maskedEmailSchema>
