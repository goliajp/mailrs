import { toast } from '@goliapkg/gds'
import { useQuery } from '@tanstack/react-query'

import { tNow } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { invalidateMail } from '@/lib/mail-cache'
import { invalidateSenderLists, sendersQuery } from '@/lib/sender-lists'
import { wireBlockSender, wireUnblockSender } from '@/wire/endpoints/block-sender'
import { wireRemoveSender } from '@/wire/endpoints/settings'

const EMPTY: readonly string[] = []

export type BlockSender = {
  /** On the reader's whitelist now: blocking takes it off, and the confirmation says so. */
  allowed: boolean
  /** Blacklist it and move the conversation to Junk, with an undo in the toast. */
  block: () => Promise<void>
  /** On the reader's blacklist now: the menu offers Unblock instead. */
  blocked: boolean
  /** Both lists are read: until then neither Block nor Unblock is known to be right. */
  ready: boolean
  /** Take it off the blacklist; the conversation stays where it is. */
  unblock: () => Promise<void>
}

/**
 * Block or unblock the sender of one message, from that message.
 *
 * `address` is the bare `From:` address. The lists are read only while
 * `enabled` (the menu is open), from the cache Settings → Senders uses.
 */
export function useBlockSender(threadId: string, address: string, enabled: boolean): BlockSender {
  const addr = address.trim().toLowerCase()
  const on = enabled && addr !== ''
  const blackQuery = useQuery(sendersQuery('blacklist', on))
  const whiteQuery = useQuery(sendersQuery('whitelist', on))
  const blacklist = blackQuery.data ?? EMPTY
  const whitelist = whiteQuery.data ?? EMPTY

  const block = async () => {
    try {
      const answer = await wireBlockSender(threadId, addr)
      invalidateSenderLists()
      invalidateMail()
      toast.success(tNow('block.done', { address: answer.address }), {
        action: {
          label: tNow('common.undo'),
          onClick: () => void undo(threadId, answer.address, answer.was_allowed),
        },
        duration: 8000,
      })
    } catch (e) {
      toast.error(errorText(e, tNow('common.failed')))
    }
  }

  const unblock = async () => {
    try {
      await wireRemoveSender('blacklist', addr)
      invalidateSenderLists()
      toast.success(tNow('block.unblocked', { address: addr }))
    } catch (e) {
      toast.error(errorText(e, tNow('common.failed')))
    }
  }

  return {
    allowed: whitelist.includes(addr),
    block,
    blocked: blacklist.includes(addr),
    ready: blackQuery.isSuccess && whiteQuery.isSuccess,
    unblock,
  }
}

async function undo(threadId: string, address: string, allow: boolean): Promise<void> {
  try {
    await wireUnblockSender(threadId, address, allow)
    invalidateSenderLists()
    invalidateMail()
    toast.success(tNow('block.undone', { address }))
  } catch (e) {
    toast.error(errorText(e, tNow('common.failed')))
  }
}
