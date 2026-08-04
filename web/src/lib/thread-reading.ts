import type { ThreadMessage } from '@/lib/types'

import { extractEmail } from '@/lib/avatar'

/**
 * Which message a thread opens on when the reader has not picked one.
 *
 * The **last received** message, not simply the last one: after you
 * reply, the tail is your own copy, and opening an Inbox thread onto
 * your own words reads like a sent mail in the Inbox (2026-07-18). A
 * thread with nothing but your own messages falls back to its tail.
 *
 * `null` means — and only means — that the thread has no messages. It
 * does not mean "nothing is selected": a thread that has messages
 * always has one to show, which is what stops the pane from rendering
 * "Select a message to preview" underneath a header and a timeline that
 * are both showing that very thread.
 */
export function defaultReadingTarget(
  messages: readonly ThreadMessage[],
  myAddress: string
): null | number {
  if (messages.length === 0) return null
  for (let i = messages.length - 1; i >= 0; i--) {
    if (extractEmail(messages[i].sender) !== myAddress) return i
  }
  return messages.length - 1
}
