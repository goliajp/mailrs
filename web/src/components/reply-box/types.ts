export type ReplyMode = 'forward' | 'reply' | 'reply-all'

export const MODE_LABELS: Record<ReplyMode, string> = {
  forward: 'Forward',
  reply: 'Reply',
  'reply-all': 'Reply All',
}
