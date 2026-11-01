import type { ThreadMessage } from '@/lib/types'

import { useCallback, useState } from 'react'

import { defaultExpanded } from './stack-model'

type State = {
  expanded: Set<number>
  /** The message last asked for, so asking again opens it again. */
  focused: null | number
  /** Every message seen in this conversation, so a new one can be told apart. */
  known: Set<number>
  showAll: boolean
  thread: null | string
}

const EMPTY: State = {
  expanded: new Set(),
  focused: null,
  known: new Set(),
  showAll: false,
  thread: null,
}

/**
 * Which messages of the open conversation are expanded.
 *
 * Decided once, when the conversation's messages first arrive: marking it
 * read and fetching it again must not fold away what was open because it
 * was unread. A message that arrives while it is open is opened, and so is
 * the one the reader is sent to.
 */
export function useStackState(
  threadId: null | string,
  messages: readonly ThreadMessage[],
  focusedUid: null | number
) {
  const [state, setState] = useState<State>(EMPTY)
  // adjusted while rendering, so the first paint of a conversation is
  // already the right one rather than everything folded for a frame
  if (messages.length > 0) {
    const next = adjust(state, threadId, messages, focusedUid)
    if (next !== state) setState(next)
  }
  const toggle = useCallback((uid: number) => {
    setState((s) => {
      const expanded = new Set(s.expanded)
      if (expanded.has(uid)) expanded.delete(uid)
      else expanded.add(uid)
      return { ...s, expanded }
    })
  }, [])
  const showAll = useCallback(() => setState((s) => ({ ...s, showAll: true })), [])
  return { expanded: state.expanded, showAll, showingAll: state.showAll, toggle }
}

function adjust(
  s: State,
  threadId: null | string,
  messages: readonly ThreadMessage[],
  focusedUid: null | number
): State {
  if (s.thread !== threadId) {
    return {
      expanded: defaultExpanded(messages, focusedUid),
      focused: focusedUid,
      known: new Set(messages.map((m) => m.uid)),
      showAll: false,
      thread: threadId,
    }
  }
  const arrived = messages.filter((m) => !s.known.has(m.uid))
  const refocused = focusedUid !== null && focusedUid !== s.focused
  if (arrived.length === 0 && !refocused) return s
  const expanded = new Set(s.expanded)
  const known = new Set(s.known)
  for (const m of arrived) {
    expanded.add(m.uid)
    known.add(m.uid)
  }
  if (refocused) expanded.add(focusedUid)
  return { ...s, expanded, focused: focusedUid, known, showAll: s.showAll || refocused }
}
