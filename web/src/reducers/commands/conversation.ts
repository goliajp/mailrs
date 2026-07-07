/**
 * Conversation commands — layer 3 (see RFC §2.3, §3.1).
 *
 * Every user-initiated conversation-list mutation flows through one of
 * these command handlers. A handler:
 *
 * 1. Snapshots every conversation-list cache line for rollback.
 * 2. Optimistically patches EVERY cache line containing the target
 *    thread — this is what makes cross-screen updates consistent.
 * 3. Calls the wire endpoint.
 * 4. On success, invalidates the entity's list keys to reconcile with
 *    server truth; on error, restores the snapshot.
 *
 * A screen never patches the cache directly — it dispatches one of
 * these commands via `useConversationCommands()`.
 */

import type { ThreadId, ThreadSummary } from '@/domain'
import type { QueryClient } from '@tanstack/react-query'

import { OPTIMISTIC_VERSION } from '@/domain'
import { deleteJson, postJson } from '@/lib/api'
import { conversationKeys } from '@/store/query-keys-v21'

import { patchMatching, restoreSnapshot, type Snapshot, snapshotMatching } from '../snapshot'

// ── shared list-response shape from the wire layer ─────────────────

type ListPayload = { readonly items: readonly ThreadSummary[] }

/**
 * Mark a thread as read across every list projection.
 * The wire endpoint is `POST /conversations/{id}/read` (per
 * `web/src/hooks/use-mail-mutations.ts::useMarkReadMutation`), kept
 * for now during the migration.
 */
export async function markThreadRead(
  qc: QueryClient,
  threadId: ThreadId,
  domains?: readonly string[]
): Promise<void> {
  const snap = snapshotMatching(qc, conversationKeys.lists())
  patchMatching<ListPayload>(qc, conversationKeys.lists(), (data) =>
    patchThread(data, threadId, { unreadCount: 0 })
  )
  try {
    const q =
      domains && domains.length > 0 ? `?domains=${encodeURIComponent(domains.join(','))}` : ''
    await postJson(`/conversations/${encodeURIComponent(threadId)}/read${q}`, {})
    qc.invalidateQueries({ queryKey: conversationKeys.lists() })
  } catch (err) {
    restoreSnapshot(qc, snap)
    throw err
  }
}

/** Mirror of `markThreadRead` — restores unread state. */
export async function markThreadUnread(
  qc: QueryClient,
  threadId: ThreadId,
  domains?: readonly string[]
): Promise<void> {
  const snap: Snapshot = snapshotMatching(qc, conversationKeys.lists())
  patchMatching<ListPayload>(qc, conversationKeys.lists(), (data) =>
    patchThread(data, threadId, { unreadCount: Math.max(1, threadUnreadOr1(data, threadId)) })
  )
  try {
    const q =
      domains && domains.length > 0 ? `?domains=${encodeURIComponent(domains.join(','))}` : ''
    await postJson(`/conversations/${encodeURIComponent(threadId)}/unread${q}`, {})
    qc.invalidateQueries({ queryKey: conversationKeys.lists() })
  } catch (err) {
    restoreSnapshot(qc, snap)
    throw err
  }
}

export async function starThread(
  qc: QueryClient,
  threadId: ThreadId,
  starred: boolean
): Promise<void> {
  const snap = snapshotMatching(qc, conversationKeys.lists())
  patchMatching<ListPayload>(qc, conversationKeys.lists(), (data) =>
    patchThread(data, threadId, { starred })
  )
  try {
    if (starred) {
      await postJson(`/conversations/${encodeURIComponent(threadId)}/star`, {})
    } else {
      await deleteJson(`/conversations/${encodeURIComponent(threadId)}/star`)
    }
    qc.invalidateQueries({ queryKey: conversationKeys.lists() })
  } catch (err) {
    restoreSnapshot(qc, snap)
    throw err
  }
}

function patchThread(
  data: ListPayload,
  threadId: ThreadId,
  patch: Partial<ThreadSummary>
): ListPayload {
  let changed = false
  const items = data.items.map((t) => {
    if (t.threadId !== threadId) return t
    changed = true
    return { ...t, ...patch, version: OPTIMISTIC_VERSION }
  })
  return changed ? { items } : data
}

// ── local helpers ───────────────────────────────────────────────────

function threadUnreadOr1(data: ListPayload, threadId: ThreadId): number {
  const t = data.items.find((x) => x.threadId === threadId)
  return t?.unreadCount ?? 1
}
