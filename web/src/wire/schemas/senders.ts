import { z } from '@/lib/zod'

// Backend: crates/webapi/src/handlers/senders.rs (AcceptSendersResponse)
export const acceptSendersSchema = z.object({
  accepted: z.number().int().min(0),
})

export type WireAcceptSenders = z.infer<typeof acceptSendersSchema>
