import type { ReplyMode } from './types'
import type { ComposerStart, ReplyContext } from '@/components/composer/start'

import { useMemo, useState } from 'react'

import { ComposerView } from '@/components/composer/composer'
import { useComposer } from '@/components/composer/use-composer'
import { useComposerAi } from '@/components/composer/use-composer-ai'

import { ModeToggle } from './mode-toggle'

type ReplyBoxProps = Omit<
  ReplyContext,
  'forwardAttachmentsUid' | 'forwardMessageId' | 'originalHtmlBody'
> & {
  forwardAttachmentsUid?: null | number
  forwardMessageId?: null | string
  mode: ReplyMode
  onModeChange: (mode: ReplyMode) => void
  onSent: () => void
  originalHtmlBody?: null | string
}

/**
 * The inline shell: the composer at the foot of a conversation. Reply,
 * reply all and forward are its modes; the recipients, Cc/Bcc, From and
 * subject fold away until asked for, and forwarding opens them.
 */
export function ReplyBox(props: ReplyBoxProps) {
  // another conversation is another compose session
  return <InlineReply key={props.threadId} {...props} />
}

function InlineReply({ mode, onModeChange, onSent, ...rest }: ReplyBoxProps) {
  const ctx: ReplyContext = {
    ...rest,
    forwardAttachmentsUid: rest.forwardAttachmentsUid ?? null,
    forwardMessageId: rest.forwardMessageId ?? null,
    originalHtmlBody: rest.originalHtmlBody ?? null,
  }
  const start = useInlineStart(ctx, mode)
  const [open, setOpen] = useState(false)
  const c = useComposer(start, {
    onSent: () => {
      setOpen(false)
      onSent()
    },
  })
  const ai = useComposerAi(c)
  const fieldsOpen = open || mode === 'forward'
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <ModeToggle
        fieldsOpen={fieldsOpen}
        mode={mode}
        onChange={(m) => {
          onModeChange(m)
          ai.dismissSuggestions()
          c.setError('')
        }}
        onToggleFields={() => setOpen((v) => !v)}
        recipients={c.to}
      />
      <ComposerView ai={ai} c={c} compact fieldsOpen={fieldsOpen} focusTo />
    </div>
  )
}

// one start per mode and thread: a new object every render would read as a
// mode change and reset what was typed into the fields
function useInlineStart(ctx: ReplyContext, mode: ReplyMode): ComposerStart {
  const {
    accountId,
    forwardAttachmentsUid,
    forwardMessageId,
    lastMessageId,
    originalBody,
    originalDate,
    originalFrom,
    originalHtmlBody,
    replyAllRecipients,
    replyRecipients,
    subject,
    threadId,
  } = ctx
  return useMemo(
    () => ({
      ctx: {
        accountId,
        forwardAttachmentsUid,
        forwardMessageId,
        lastMessageId,
        originalBody,
        originalDate,
        originalFrom,
        originalHtmlBody,
        replyAllRecipients,
        replyRecipients,
        subject,
        threadId,
      },
      kind: 'inline',
      mode,
    }),
    [
      accountId,
      forwardAttachmentsUid,
      forwardMessageId,
      lastMessageId,
      mode,
      originalBody,
      originalDate,
      originalFrom,
      originalHtmlBody,
      replyAllRecipients,
      replyRecipients,
      subject,
      threadId,
    ]
  )
}
