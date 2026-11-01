import { toast } from '@goliapkg/gds'

import { tNow } from '@/i18n'
import { errorText } from '@/lib/error-text'
import { findConversation, invalidateMail } from '@/lib/mail-cache'
import { wireReclassifySender, wireUndoCorrection } from '@/wire/endpoints/sender-rules'
import { type SenderBucket } from '@/wire/schemas/sender-rules'

/** The bucket each correcting action files into. */
export const CORRECTION_BUCKET: Record<string, SenderBucket> = {
  'mark-notification': 'notifications',
  'mark-promotion': 'promotions',
  'move-to-inbox': 'inbox',
}

/**
 * After one conversation was corrected: offer to do the same for the sender's
 * others and remember it, then offer to undo that. Nothing wider happens
 * unless the reader asks.
 */
export function offerSenderCorrection(threadId: string, action: string, self: string): void {
  const bucket = CORRECTION_BUCKET[action]
  const sender = senderOf(threadId, self)
  if (!bucket || !sender) return
  toast.info(tNow('senders.offer', { sender }), {
    action: {
      label: tNow('senders.offerAction'),
      onClick: () => void moveSender(sender, bucket),
    },
    duration: 10000,
  })
}

/** Who wrote the conversation to the reader: the first participant who is not them. */
export function senderOf(threadId: string, self: string): null | string {
  const row = findConversation(threadId)
  if (!row) return null
  const me = self.toLowerCase()
  const other = row.participants.map(bare).find((p) => p.includes('@') && p !== me)
  return other ?? null
}

/** `Name <addr>` or `addr`, as its bare address. */
function bare(participant: string): string {
  const m = /<([^>]+)>/.exec(participant)
  if (m) return m[1].trim().toLowerCase()
  return participant.trim().toLowerCase()
}

async function moveSender(sender: string, bucket: SenderBucket): Promise<void> {
  try {
    const answer = await wireReclassifySender(sender, bucket)
    invalidateMail()
    const id = answer.correction_id
    if (!id) {
      toast.success(tNow('senders.remembered', { sender }))
      return
    }
    toast.success(tNow('senders.moved', { count: answer.moved, sender }), {
      action: { label: tNow('common.undo'), onClick: () => void undo(id) },
      duration: 10000,
    })
  } catch (e) {
    toast.error(errorText(e, tNow('action.failed')))
  }
}

async function undo(id: string): Promise<void> {
  try {
    const answer = await wireUndoCorrection(id)
    invalidateMail()
    toast.success(tNow('senders.undone', { count: answer.restored }))
  } catch (e) {
    toast.error(errorText(e, tNow('action.failed')))
  }
}
