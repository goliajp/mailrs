import type { WireSentMessage } from '@/wire/schemas/mail'

// Drop any prior row with the same message_id before prepending the
// placeholder. Covers three races that would otherwise duplicate the
// row in the Sent UI:
//   1. applyOptimisticSent fires twice for one send (double-click, retry)
//   2. The invalidate-driven refetch lands between our setQueryData and
//      the next render, then this setQueryData re-fires and re-inserts
//   3. A WebSocket-driven cache update inserted the real row before
//      applyOptimisticSent runs
// message_id is unique per outbound message (server generates one per
// send), so it is the correct dedupe key. uid differs (placeholder=0,
// real row >0) which is why the prior [placeholder, ...old] pattern
// leaked duplicates as two rows in Sent.
//
// Kept in its own module so vitest can cover it without dragging the
// mutation hook's transitive import chain (RQ + wire schemas + auth
// store) into the test env.
export function dedupeSentByMessageId(
  placeholder: WireSentMessage,
  old: readonly WireSentMessage[] | undefined
): WireSentMessage[] {
  const filtered = old ? old.filter((m) => m.message_id !== placeholder.message_id) : []
  return [placeholder, ...filtered]
}
