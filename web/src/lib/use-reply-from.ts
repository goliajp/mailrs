import { useState } from 'react'

import { replyFromFor, useFromAddresses } from '@/lib/from-addresses'

/**
 * Which address the composer sends from.
 *
 * Follows the account the message arrived at as the reader moves
 * between conversations, until they pick one by hand — that choice is
 * theirs to keep for as long as the composer is open, and resetting it
 * under them while they are typing would be worse than any default.
 */
export function useReplyFrom(accountId: string | undefined): {
  from: string
  setFrom: (address: string) => void
} {
  const { addresses } = useFromAddresses()
  const [chosen, setChosen] = useState<null | string>(null)
  return { from: chosen ?? replyFromFor(accountId, addresses), setFrom: setChosen }
}
