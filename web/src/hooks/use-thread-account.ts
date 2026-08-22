import type { ConversationSummary } from '@/lib/types'

import { useQueryClient } from '@tanstack/react-query'

import { conversationKeys } from '@/store/query-keys-v21'

/**
 * Which connected mailbox a conversation arrived at.
 *
 * Read out of the list cache rather than fetched: the row is already
 * there — it is what the reader clicked — and a second request for one
 * string would be a request per conversation opened.
 *
 * `undefined` when the thread is not in any cached page, which reads
 * downstream as this server's own mail. That is the right default: it
 * is what every conversation was before connected mailboxes existed,
 * and a reply from this server is one somebody can still send.
 */
export function useThreadAccountId(threadId: null | string): string | undefined {
  const client = useQueryClient()
  if (!threadId) return undefined
  const pages = client.getQueriesData<{ pages?: ConversationSummary[][] }>({
    queryKey: conversationKeys.all(),
  })
  for (const [, data] of pages) {
    for (const page of data?.pages ?? []) {
      const hit = page.find((c) => c.thread_id === threadId)
      if (hit) return hit.account_id
    }
  }
  return undefined
}
