/**
 * Blocking a conversation's sender from one of its messages: the address
 * goes on the reader's blacklist (and off their whitelist) and the
 * conversation goes to Junk, in one call. Unblocking outside the undo is
 * the plain blacklist delete in `settings.ts`.
 */

import { wireFetch } from '../client'
import { blockSenderResponseSchema, type WireBlockSenderResponse } from '../schemas/block-sender'
import { emptyResponseSchema } from '../schemas/mutations'

export function wireBlockSender(
  threadId: string,
  address: string
): Promise<WireBlockSenderResponse> {
  return wireFetch(blockSenderResponseSchema, {
    body: { address },
    method: 'POST',
    path: `/conversations/${encodeURIComponent(threadId)}/block-sender`,
  })
}

/** The undo: off the blacklist, back to the Inbox, and back on the whitelist when `allow`. */
export async function wireUnblockSender(
  threadId: string,
  address: string,
  allow: boolean
): Promise<void> {
  await wireFetch(emptyResponseSchema, {
    allowEmpty: true,
    body: { address, allow },
    method: 'POST',
    path: `/conversations/${encodeURIComponent(threadId)}/unblock-sender`,
  })
}
