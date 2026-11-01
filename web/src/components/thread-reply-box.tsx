import type { ReplyContext } from '@/components/composer/start'

import { ReplyBox, type ReplyMode } from '@/components/reply-box'

/** What the composer needs about what is being replied to; the composer's own type. */
export type ThreadReplyContext = ReplyContext

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
