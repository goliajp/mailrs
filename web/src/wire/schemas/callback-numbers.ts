/**
 * The phone numbers a sender gave the reader before, in any conversation.
 *
 * Backend: crates/webapi/src/handlers/callback_numbers.rs — fn list
 * `number` is digits only; `first_message_id` is the earliest message that
 * gave it, so a number first given by the message being read is not old.
 */

import { z } from '@/lib/zod'

export const givenNumberSchema = z.object({
  first_message_id: z.string(),
  first_seen: z.number(),
  number: z.string(),
})

export const callbackNumbersSchema = z.object({
  numbers: z.array(givenNumberSchema),
  sender: z.string(),
})

export type WireCallbackNumbers = z.infer<typeof callbackNumbersSchema>
export type WireGivenNumber = z.infer<typeof givenNumberSchema>
