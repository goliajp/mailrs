import type { ThreadMessage } from '@/lib/types'

export type StackItem =
  | { at: number; count: number; kind: 'hidden' }
  | { expanded: boolean; index: number; kind: 'message'; message: ThreadMessage }

type MessageItem = Extract<StackItem, { kind: 'message' }>

/**
 * The messages open when a conversation is opened: the newest, every
 * unread one, and the one the reader came for.
 */
export function defaultExpanded(
  messages: readonly ThreadMessage[],
  focusedUid: null | number
): Set<number> {
  const open = new Set<number>()
  for (const m of messages) {
    if (isUnread(m)) open.add(m.uid)
  }
  const newest = messages[messages.length - 1]
  if (newest) open.add(newest.uid)
  if (focusedUid !== null && messages.some((m) => m.uid === focusedUid)) open.add(focusedUid)
  return open
}

/** Unread: the `\Seen` flag (bit 1) is not set, as the server counts it. */
export function isUnread(m: ThreadMessage): boolean {
  return (m.flags & 1) === 0
}

/** How many messages a conversation shows in full before older ones fold away. */
export const FOLD_ABOVE = 5

/** The newest messages that never fold away. */
const KEEP_NEWEST = 3

/**
 * What the stack draws, top to bottom. Over `FOLD_ABOVE` messages, each run
 * of two or more collapsed messages between the first and the newest three
 * folds into one line; an open message never does.
 */
export function stackItems(
  messages: readonly ThreadMessage[],
  expanded: ReadonlySet<number>,
  showAll: boolean
): StackItem[] {
  const foldable = (i: number) =>
    !showAll &&
    messages.length > FOLD_ABOVE &&
    i > 0 &&
    i < messages.length - KEEP_NEWEST &&
    !expanded.has(messages[i].uid)
  const out: StackItem[] = []
  let run: MessageItem[] = []
  const flush = () => {
    if (run.length > 1) out.push({ at: run[0].index, count: run.length, kind: 'hidden' })
    if (run.length === 1) out.push(run[0])
    run = []
  }
  messages.forEach((message, index) => {
    const item: MessageItem = {
      expanded: expanded.has(message.uid),
      index,
      kind: 'message',
      message,
    }
    if (foldable(index)) {
      run.push(item)
      return
    }
    flush()
    out.push(item)
  })
  flush()
  return out
}
