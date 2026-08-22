import { ReplyBox, type ReplyMode } from '@/components/reply-box'

// everything the composer needs about what is being replied to or
// forwarded. computed once in ThreadView and handed to both the desktop
// pane and the mobile modal, which used to spell out the same eleven
// props each.
export type ThreadReplyContext = {
  /**
   * The connected mailbox this conversation arrived at. Part of the
   * shared context so the desktop pane and the phone cannot disagree
   * about which address a reply leaves by.
   */
  accountId?: string
  forwardAttachmentsUid: null | number
  forwardMessageId: null | string
  lastMessageId: string
  originalBody: string
  originalDate: string
  originalFrom: string
  originalHtmlBody: null | string
  replyAllRecipients: string
  replyRecipients: string
  subject: string
  threadId: string
}

export function ThreadReplyBox({
  ctx,
  mode,
  onModeChange,
  onSent,
}: {
  ctx: ThreadReplyContext
  mode: ReplyMode
  onModeChange: (m: ReplyMode) => void
  onSent: () => void
}) {
  return <ReplyBox {...ctx} mode={mode} onModeChange={onModeChange} onSent={onSent} />
}
