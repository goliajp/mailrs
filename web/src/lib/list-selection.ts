import type { MailListId } from '@/lib/mail-lists'

/** A row the user explicitly clicked, and the list they clicked it in. */
export type PickedItem = SelectableRow & { list: MailListId }

/**
 * One row of whatever list is showing, reduced to what the reading pane
 * needs: which thread to open, and which message inside it to focus.
 *
 * `uid` is null for a conversation row — the thread opens at its own
 * reading position — and set for a Send row, which is one message.
 */
export type SelectableRow = {
  threadId: string
  uid: null | number
}

/**
 * The current item of `list`.
 *
 * Derived, never stored. Four components used to keep their own opinion
 * of this in mount and unmount effects on one global atom — `chat.tsx`
 * ran a second conversation query and took its first row even while the
 * Send list was on screen, the conversation list cleared and re-picked
 * on every identity change, the Send list picked on mount and cleared on
 * unmount, and the Draft list cleared on mount. Which one won came down
 * to mount order, so switching tabs left the reading pane showing a
 * thread from the list you had just left.
 *
 * The rule, in one place:
 *
 * - a pick counts only inside the list it was made in, and only while
 *   that row is still there;
 * - otherwise the list's first row;
 * - and null when there are none.
 *
 * The `list` check is not redundant with clearing the pick on a tab
 * switch: a thread you replied in is in both Inbox and Send, so a pick
 * carried across would look valid. A derivation that depends on a latch
 * having been reset is a latch.
 */
export function resolveSelection(
  list: MailListId,
  picked: null | PickedItem,
  rows: readonly SelectableRow[]
): null | SelectableRow {
  if (picked !== null && picked.list === list) {
    const held = rows.find((r) => r.threadId === picked.threadId)
    if (held) return picked
  }
  return rows[0] ?? null
}
