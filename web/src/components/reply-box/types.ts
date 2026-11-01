import type { ReplyMode } from '@/components/composer/start'
import type { Key } from '@/i18n'

export type { ReplyMode }

export const MODE_LABELS: Record<ReplyMode, Key> = {
  forward: 'compose.mode.forward',
  reply: 'compose.mode.reply',
  'reply-all': 'compose.mode.replyAll',
}
