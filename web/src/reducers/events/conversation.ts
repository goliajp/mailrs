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

import { extractEmail } from '@/lib/avatar'
import { patchCachedList } from '@/lib/patch-cached-list'
import { dashboardKeys, mailKeys } from '@/lib/query-keys'
import { conversationKeys } from '@/store/query-keys-v21'

type ThreadLocation = { readonly idx: number; readonly page: number }

/** Within this long of the tab's own change, an echo of it is presumed. */
export const LOCAL_ECHO_MS = 5000

/**
 * `MailboxChanged` — the mailbox moved under the reader, from wherever.
 * The counts always refetch. The scrolling lists refetch too, unless this
 * tab patched them itself a moment ago: then the event is most likely the
 * echo of that change, which the patch already shows.
 */
export function onMailboxChanged(qc: QueryClient, now: number, localChangeAt: number): void {
  refreshMailState(qc, { lists: now - localChangeAt > LOCAL_ECHO_MS })
}

/**
 * `NewMessage` arrived for the current user.
 *
 * If the target thread already lives in a cached list, we patch it in
 * place and lift it to the top of page 0 — zero network. If the
 * thread is unknown to a given cache (e.g. it doesn't match that
 * filter yet), invalidate only that specific query key so the server
 * decides whether the row belongs.
 *
 * **Unless the user sent it.** `record_message_arrival` takes `is_own`
 * and deliberately leaves the thread's date, position and unread count
 * alone — "replying must not re-date the Inbox row to the user's own
 * send time". This reducer did all three anyway, so a reply jumped its
 * thread to the top of the Inbox and marked it unread until the next
 * refresh put the server's answer back.
 *
 * The badge and the other single-page lists count unread mail, so they
 * refetch: the event says a message arrived, not what the counts are.
 */
export function onNewMessage(qc: QueryClient, event: NewMessageEvent): void {
  qc.invalidateQueries({ queryKey: conversationKeys.lists() }).catch(() => {})
  const entries = qc.getQueriesData<InfiniteData<ConversationSummary[]>>({
    queryKey: conversationKeys.infinites(),
  })
  for (const [key, data] of entries) {
    if (!data) continue
    const location = locateThread(data, event.thread_id)
    if (location === null) {
      qc.invalidateQueries({ exact: true, queryKey: key }).catch(() => {})
      continue
    }
    patchCachedList(qc, key, liftAndPatch(data, location, event))
  }
}

// ── local helpers ───────────────────────────────────────────────────

/**
 * Refetch what the reader is shown from the server: the badge and the
 * other single-page lists, the category and dashboard counts, and — with
 * `lists` — the scrolling lists too.
 */
export function refreshMailState(qc: QueryClient, opts: { lists: boolean }): void {
  qc.invalidateQueries({ queryKey: conversationKeys.lists() }).catch(() => {})
  qc.invalidateQueries({ queryKey: mailKeys.categories([]) }).catch(() => {})
  qc.invalidateQueries({ queryKey: dashboardKeys.all() }).catch(() => {})
  if (opts.lists) {
    qc.invalidateQueries({ queryKey: conversationKeys.infinites() }).catch(() => {})
  }
}

/**
 * True when the event describes the user's own outbound copy.
 *
 * The same question the server asks — `senders_csv_contains_user` — and
 * it has to be asked here too, because the answer changes what the row
 * is allowed to do. `mirror_send` publishes this event with
 * `sender = the user's own address`.
 */
function isOwnSend(event: NewMessageEvent): boolean {
  const from = extractEmail(event.sender).toLowerCase()
  return from.length > 0 && from === event.user.trim().toLowerCase()
}

function liftAndPatch(
  old: InfiniteData<ConversationSummary[]>,
  location: ThreadLocation,
  event: NewMessageEvent
): InfiniteData<ConversationSummary[]> {
  const existing = old.pages[location.page]?.[location.idx]
  if (!existing) return old

  if (isOwnSend(event)) {
    // Nothing. `record_message_arrival`'s `is_own` branch moves only
    // `count` and `sent_count`, and those are counters — see below for
    // why this reducer does not compute counters. The row's date,
    // unread count and position must not move for a send, and the two
    // numbers that do move arrive with the invalidation
    // `applyOptimisticSent` already issues.
    return old
  }

  // Set, never increment. The event carries a thread id and no message
  // identity, so a client cannot tell a second delivery of one event
  // from a second message — and the stream is at-least-once: the feed
  // consumer re-tails on `FEEDRESYNC` and on any read error, and the
  // socket reconnects on its own. Incrementing turned that into
  // permanent over-counting: 25 crates.io notifications arrived in a
  // burst on 2026-08-04 and three of them showed "3" against a server
  // that said 1 for all 57 rows — and nothing on the server side ever
  // decrements `count`, so it had never been anything but 1.
  //
  // `unread_count` becomes at-least-one because that is what the event
  // actually tells us: there is mail here you have not read. The exact
  // counts belong to the server and arrive with the next fetch; showing
  // a number the client cannot compute is worse than showing a stale
  // one, because the stale one converges.
  const patched: ConversationSummary = {
    ...existing,
    last_date: Math.floor(Date.now() / 1000),
    snippet: event.snippet,
    subject: event.subject || existing.subject,
    unread_count: Math.max(existing.unread_count, 1),
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
