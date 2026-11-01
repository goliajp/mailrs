import { type Key } from '@/i18n'
export type ReplyMode = 'forward' | 'reply' | 'reply-all'

export const MODE_LABELS: Record<ReplyMode, Key> = {
  forward: 'compose.mode.forward',
  reply: 'compose.mode.reply',
  'reply-all': 'compose.mode.replyAll',
}
