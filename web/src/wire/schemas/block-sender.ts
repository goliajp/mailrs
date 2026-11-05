// Backend: crates/webapi/src/handlers/block_sender.rs — fn block_sender
// (`Json(BlockSenderResponse { address, was_allowed })`).

import { z } from '@/lib/zod'

export const blockSenderResponseSchema = z.object({
  /** As stored: trimmed and lowercased. */
  address: z.string(),
  /** It was on the whitelist and was taken off; the undo puts it back. */
  was_allowed: z.boolean(),
})

export type WireBlockSenderResponse = z.infer<typeof blockSenderResponseSchema>
