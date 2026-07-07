/**
 * Server-push event reducers — layer 3 (see RFC §2.3, §3.2).
 *
 * Every WebSocket event on the `/api/events` stream is dispatched
 * through one of these pure(-ish) reducers. `useMailEvents` in the
 * hook layer subscribes to the socket and calls the reducer; every
 * cache mutation happens here, never scattered across the socket
 * onmessage callback.
 *
 * These functions accept a `QueryClient` because RQ is our store —
 * calling them with a mock client in a test is straightforward and
 * exercises the exact code path production runs.
 */

import type { ConversationSummary, NewMessageEvent } from '@/lib/types'
import type { InfiniteData, QueryClient } from '@tanstack/react-query'

import { mailKeys } from '@/lib/query-keys'
import { conversationKeys } from '@/store/query-keys-v21'

type ThreadLocation = { readonly idx: number; readonly page: number }

/**
 * `ConversationRead` — the server acknowledged (or another device
 * initiated) marking a thread as read. Sync every cached list line
 * containing the thread to `unread_count = 0`.
 */
export function onConversationRead(qc: QueryClient, threadId: string): void {
  patchListsForThread(qc, threadId, (c) => ({ ...c, unread_count: 0 }))
  // Also broaden invalidation to the single-page `list` prefix so the
  // dashboard's `conversationKeys.list({folder:'INBOX'})` reader
  // recomputes its derived unread count on the next tick.
  qc.invalidateQueries({ queryKey: conversationKeys.lists() }).catch(() => {})
}

// ── local helpers ───────────────────────────────────────────────────

/**
 * `NewMessage` arrived for the current user.
 *
 * If the target thread already lives in a cached list, we patch it in
 * place and lift it to the top of page 0 — zero network. If the
 * thread is unknown to a given cache (e.g. it doesn't match that
 * filter yet), invalidate only that specific query key so the server
 * decides whether the row belongs.
 *
 * Iterates BOTH the legacy `mailKeys.conversations()` prefix and the
 * v2.1 `conversationKeys.infinites()` prefix so every reader across
 * the migration transition sees the same live update.
 */
export function onNewMessage(qc: QueryClient, event: NewMessageEvent): void {
  const entries = [
    ...qc.getQueriesData<InfiniteData<ConversationSummary[]>>({
      queryKey: mailKeys.conversations(),
    }),
    ...qc.getQueriesData<InfiniteData<ConversationSummary[]>>({
      queryKey: conversationKeys.infinites(),
    }),
  ]
  for (const [key, data] of entries) {
    if (!data) continue
    const location = locateThread(data, event.thread_id)
    if (location === null) {
      qc.invalidateQueries({ exact: true, queryKey: key }).catch(() => {})
      continue
    }
    qc.setQueryData<InfiniteData<ConversationSummary[]>>(key, (old) =>
      old ? liftAndPatch(old, location, event) : old
    )
  }
}

function liftAndPatch(
  old: InfiniteData<ConversationSummary[]>,
  location: ThreadLocation,
  event: NewMessageEvent
): InfiniteData<ConversationSummary[]> {
  const existing = old.pages[location.page]?.[location.idx]
  if (!existing) return old
  const patched: ConversationSummary = {
    ...existing,
    last_date: Math.floor(Date.now() / 1000),
    last_sender: event.sender,
    message_count: existing.message_count + 1,
    received_count: existing.received_count + 1,
    snippet: event.snippet,
    subject: event.subject || existing.subject,
    unread_count: existing.unread_count + 1,
  }
  const removedPages = old.pages.map((page, p) =>
    p === location.page ? page.filter((_, i) => i !== location.idx) : page
  )
  removedPages[0] = [patched, ...removedPages[0]]
  return { ...old, pages: removedPages }
}

function locateThread(
  data: InfiniteData<ConversationSummary[]>,
  threadId: string
): null | ThreadLocation {
  for (let p = 0; p < data.pages.length; p++) {
    const idx = data.pages[p].findIndex((c) => c.thread_id === threadId)
    if (idx >= 0) return { idx, page: p }
  }
  return null
}

function patchListsForThread(
  qc: QueryClient,
  threadId: string,
  patch: (c: ConversationSummary) => ConversationSummary
): void {
  const prefixes = [mailKeys.conversations(), conversationKeys.infinites()]
  for (const prefix of prefixes) {
    qc.setQueriesData<InfiniteData<ConversationSummary[]>>({ queryKey: prefix }, (old) => {
      if (!old) return old
      let touched = false
      const pages = old.pages.map((page) => {
        let pageTouched = false
        const next = page.map((c) => {
          if (c.thread_id !== threadId) return c
          pageTouched = true
          touched = true
          return patch(c)
        })
        return pageTouched ? next : page
      })
      return touched ? { ...old, pages } : old
    })
  }
}
